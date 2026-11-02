//! Real suspended calls exercise resource ownership at the host publication boundary.

use super::*;
use mod_api::drawing::DrawingPermissions;
use mod_render::retained::{self, EntityKey, GeometrySpec, Vertex};
use std::sync::{Mutex, atomic::AtomicBool};

/// Makes a triangle with extra unreferenced vertices to exercise resident allocation quotas.
fn mesh(vertices: usize) -> GeometrySpec {
    let mut mesh = vec![
        Vertex {
            position: [0.; 3],
            color: [1.; 4],
            uv: [0.; 2],
            data: [0.; 4],
        };
        vertices
    ];
    mesh[1].position = [1., 0., 0.];
    mesh[2].position = [0., 1., 0.];
    GeometrySpec::Triangles {
        vertices: mesh,
        indices: vec![0, 1, 2],
    }
}

/// Keeps every invocation valid while forcing it across several scheduling slices.
fn source() -> String {
    crate::tests::fixture(
        r#"
        (local $left i32)
        i32.const 12000 local.set $left
        (loop $work local.get $left i32.const 1 i32.sub local.tee $left br_if $work)
    "#,
        "Drawing",
    )
}

/// Waits for admission without assuming the preceding callback left no scheduling debt.
fn start_suspended(instance: &mut Instance, budget: &crate::CallbackBudget) {
    for _ in 0..256 {
        match instance.run_frame_slice().unwrap() {
            CallbackOutcome::Suspended => return,
            CallbackOutcome::Deferred => next_frame(instance, budget),
            CallbackOutcome::Executed => panic!("fixture must suspend before publication"),
        }
    }
    panic!("bounded scheduling debt prevented invocation admission")
}

#[test]
fn repeated_suspended_geometry_and_material_replacements_release_renderer_retirements() {
    let (mut instance, _ticker, budget) = scheduled(&source(), 2_000_000);
    let replace = Arc::new(AtomicBool::new(false));
    let enter = Arc::clone(&replace);
    let handles = Arc::new(Mutex::new(None));
    let previous = Arc::clone(&handles);
    let vertices = mod_api::MAX_DRAW_MUTATION_BYTES / size_of::<Vertex>() / 2;
    instance.store.call_hook(move |mut store, hook| {
        if matches!(hook, wasmtime::CallHook::CallingWasm) && enter.swap(false, Ordering::Relaxed) {
            let retained = &mut store.data_mut().render.retained;
            let mut previous = previous.lock().unwrap();
            if let Some((geometry, material)) = previous.take() {
                retained.remove(geometry).unwrap();
                retained.remove(material).unwrap();
            }
            *previous = Some((
                retained.create_geometry(mesh(vertices)).unwrap(),
                retained
                    .create_material(mod_render::material::BUILTIN, DrawingPermissions::ALL)
                    .unwrap(),
            ));
        }
        Ok(())
    });
    let rounds = mod_api::MAX_DRAW_MATERIALS.max(mod_api::MAX_DRAW_VERTICES / vertices) + 3;
    let mut released = None;
    for _ in 0..rounds {
        next_frame(&mut instance, &budget);
        if let Some((geometry, material)) = released.take() {
            assert!(std::sync::Weak::upgrade(&geometry).is_none());
            assert!(std::sync::Weak::upgrade(&material).is_none());
        }
        let renderer = Arc::clone(instance.store.data().render.retained.snapshot());
        let old = handles.lock().unwrap().map(|(geometry, material)| {
            (
                Arc::downgrade(&renderer.geometry[&geometry]),
                Arc::downgrade(&renderer.materials[&material]),
            )
        });
        replace.store(true, Ordering::Relaxed);
        start_suspended(&mut instance, &budget);
        assert!(Arc::ptr_eq(
            instance.store.data().render.retained.snapshot(),
            &renderer
        ));
        finish(&mut instance, &budget).unwrap();
        let published = instance.store.data().render.retained.snapshot();
        assert_eq!(published.geometry.len(), 1);
        assert_eq!(published.materials.len(), 1);
        assert_ne!(Arc::as_ptr(published), Arc::as_ptr(&renderer));
        if let Some((geometry, material)) = &old {
            assert!(geometry.upgrade().is_some());
            assert!(material.upgrade().is_some());
        }
        drop(renderer);
        released = old;
    }
    next_frame(&mut instance, &budget);
    let (geometry, material) = released.unwrap();
    assert!(geometry.upgrade().is_none());
    assert!(material.upgrade().is_none());
}

/// Makes a retained placement anchored to a specific observed actor handle.
fn drawing(
    geometry: mod_api::drawing::DrawingHandle,
    entity: mod_api::drawing::DrawingHandle,
) -> retained::Instance {
    retained::Instance {
        geometry,
        material: None,
        clip: None,
        params: [0.; mod_api::materials::PARAMS],
        anchor: retained::Anchor::Entity(retained::EntityAnchor {
            entity,
            body_rotation: true,
        }),
        transform: Default::default(),
        tint: [1.; 4],
        depth: Default::default(),
        blend: Default::default(),
        width: 1.,
        width_mode: Default::default(),
        join: Default::default(),
        cap: Default::default(),
    }
}

#[test]
fn new_actor_references_survive_culling_but_confirmed_despawns_remove_anchored_drawings() {
    let actor = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    };
    for despawn_before_publication in [false, true] {
        let (mut instance, _ticker, budget) = scheduled(&source(), 2_000_000);
        instance
            .store
            .data_mut()
            .render
            .retained
            .bind_session(actor.session, actor.dimension);
        instance.update_drawing_entities(vec![actor], &[]).unwrap();
        let observed = Arc::new(Mutex::new(None));
        let capture = Arc::clone(&observed);
        instance.store.call_hook(move |mut store, hook| {
            if matches!(hook, wasmtime::CallHook::CallingWasm) {
                let mut observed = capture.lock().unwrap();
                if observed.is_none() {
                    let retained = &mut store.data_mut().render.retained;
                    let entity = retained.observe_entities().unwrap()[0].0;
                    let geometry = retained.create_geometry(mesh(3)).unwrap();
                    let instance = retained
                        .create(drawing(geometry, entity), DrawingPermissions::ALL)
                        .unwrap();
                    *observed = Some((entity, instance));
                }
            }
            Ok(())
        });
        start_suspended(&mut instance, &budget);
        assert!(
            instance
                .store
                .data()
                .render
                .retained
                .snapshot()
                .entities
                .is_empty()
        );
        assert_eq!(instance.drawing_entity_keys().collect::<Vec<_>>(), [actor]);
        instance.update_drawing_entities(Vec::new(), &[]).unwrap();
        if despawn_before_publication {
            // Native removal must still reach references absent from both published output and candidates.
            instance
                .update_drawing_entities(Vec::new(), &[actor])
                .unwrap();
        }
        finish(&mut instance, &budget).unwrap();
        let (entity, drawing) = observed.lock().unwrap().unwrap();
        let published = instance.store.data().render.retained.snapshot();
        assert_eq!(
            published.entities.get(&entity).copied(),
            (!despawn_before_publication).then_some(actor)
        );
        assert_eq!(
            published.instances.contains_key(&drawing),
            !despawn_before_publication
        );
        if !despawn_before_publication {
            instance
                .update_drawing_entities(Vec::new(), &[actor])
                .unwrap();
            let published = instance.store.data().render.retained.snapshot();
            assert!(published.entities.is_empty());
            assert!(published.instances.is_empty());
        }
    }
}

#[test]
fn retained_masks_survive_culling_across_suspension_but_not_confirmed_despawns() {
    let actor = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    };
    for staged_selection in [false, true] {
        for despawn in [false, true] {
            let (mut instance, _ticker, budget) = scheduled(&source(), 2_000_000);
            let masks = &mut instance.store.data_mut().render.masks;
            masks.bind(mod_api::drawing::DrawingSession {
                connection: actor.session,
                dimension: actor.dimension,
                local: true,
                permissions: DrawingPermissions::ALL,
            });
            masks.publish_targets(&[(actor.into(), 36)]);
            let target = masks.entity(actor.runtime_id).unwrap();
            let mask = masks.create(mod_api::drawing::MaskDepth::Test).unwrap();
            if !staged_selection {
                masks.select(mask, &[target]).unwrap();
            }
            instance.store.data_mut().render.commit_retained();
            let committed = Arc::clone(&instance.store.data().render.output().masks);
            instance.store.call_hook(move |mut store, hook| {
                if staged_selection && matches!(hook, wasmtime::CallHook::CallingWasm) {
                    store
                        .data_mut()
                        .render
                        .masks
                        .select(mask, &[target])
                        .unwrap();
                }
                Ok(())
            });
            start_suspended(&mut instance, &budget);
            assert!(instance.drawing_entity_keys().any(|key| key == actor));
            instance.publish_mask_targets(&[]);
            instance.update_drawing_entities(Vec::new(), &[]).unwrap();
            assert!(Arc::ptr_eq(
                &committed,
                &instance.store.data().render.output().masks
            ));
            if despawn {
                instance
                    .update_drawing_entities(Vec::new(), &[actor])
                    .unwrap();
            }
            finish(&mut instance, &budget).unwrap();
            let published = &instance.store.data().render.output().masks;
            assert_eq!(published[0].handle, mask);
            assert_eq!(
                published[0].entities,
                if despawn { vec![] } else { vec![actor.into()] }
            );
            if !despawn {
                instance.publish_mask_targets(&[(actor.into(), 36)]);
                assert_eq!(
                    instance
                        .store
                        .data_mut()
                        .render
                        .masks
                        .entity(actor.runtime_id)
                        .unwrap(),
                    target
                );
                instance
                    .update_drawing_entities(Vec::new(), &[actor])
                    .unwrap();
                assert!(
                    instance.store.data().render.output().masks[0]
                        .entities
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn suspended_mask_target_creation_uses_shared_confirmed_removal_tracking() {
    let actor = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 7,
        spawn_revision: 1,
    };
    for despawn in [false, true] {
        let (mut instance, _ticker, budget) = scheduled(&source(), 2_000_000);
        let masks = &mut instance.store.data_mut().render.masks;
        masks.bind(mod_api::drawing::DrawingSession {
            connection: actor.session,
            dimension: actor.dimension,
            local: true,
            permissions: DrawingPermissions::ALL,
        });
        masks.publish_targets(&[(actor.into(), 36)]);
        let observed = Arc::new(Mutex::new(None));
        let capture = Arc::clone(&observed);
        instance.store.call_hook(move |mut store, hook| {
            if matches!(hook, wasmtime::CallHook::CallingWasm) {
                let mut observed = capture.lock().unwrap();
                if observed.is_none() {
                    let masks = &mut store.data_mut().render.masks;
                    let target = masks.entity(actor.runtime_id).unwrap();
                    let mask = masks.create(mod_api::drawing::MaskDepth::Test).unwrap();
                    masks.select(mask, &[target]).unwrap();
                    *observed = Some((target, mask));
                }
            }
            Ok(())
        });
        start_suspended(&mut instance, &budget);
        instance.publish_mask_targets(&[]);
        instance.update_drawing_entities(Vec::new(), &[]).unwrap();
        assert!(instance.drawing_entity_keys().any(|key| key == actor));
        assert!(instance.store.data().render.output().masks.is_empty());
        if despawn {
            instance
                .update_drawing_entities(Vec::new(), &[actor])
                .unwrap();
        }
        finish(&mut instance, &budget).unwrap();
        let (target, mask) = observed.lock().unwrap().unwrap();
        let published = &instance.store.data().render.output().masks;
        assert_eq!(published[0].handle, mask);
        assert_eq!(
            published[0].entities,
            if despawn { vec![] } else { vec![actor.into()] }
        );
        let masks = &mut instance.store.data_mut().render.masks;
        assert_eq!(masks.select(mask, &[target]).is_ok(), !despawn);
    }
}
