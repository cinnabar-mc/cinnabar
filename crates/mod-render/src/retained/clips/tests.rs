use super::*;

/// Supplies a mask placement whose geometry belongs to the test store.
fn clip(geometry: Handle, parent: Option<Handle>) -> Clip {
    Clip {
        status: Default::default(),
        geometry,
        anchor: Anchor::Camera,
        transform: Transform::default(),
        parent,
    }
}

#[test]
fn clips_enforce_authority_ownership_nesting_and_resource_lifetimes() {
    let mut store = Store::default();
    let geometry = store.create_geometry(super::super::tests::mesh()).unwrap();
    assert_eq!(
        store
            .create_clip(clip(geometry, None), DrawingPermissions::default())
            .unwrap_err()
            .code,
        ErrorCode::Denied
    );
    let parent = store
        .create_clip(clip(geometry, None), DrawingPermissions::ALL)
        .unwrap();
    let child = store
        .create_clip(clip(geometry, Some(parent)), DrawingPermissions::ALL)
        .unwrap();
    assert_eq!(
        store
            .create_clip(clip(geometry, Some(child)), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    let mut instance = super::super::tests::drawing(geometry);
    instance.clip = Some(child);
    let id = store.create(instance, DrawingPermissions::ALL).unwrap();
    assert_eq!(store.remove(child).unwrap_err().code, ErrorCode::InUse);
    assert_eq!(store.remove(parent).unwrap_err().code, ErrorCode::InUse);
    assert_eq!(store.remove(geometry).unwrap_err().code, ErrorCode::InUse);
    store.remove(id).unwrap();
    store.remove(child).unwrap();
    store.remove(parent).unwrap();
    store.remove(geometry).unwrap();
    store.commit();
    assert!(store.snapshot().geometry.is_empty());
    assert!(store.snapshot().clips.is_empty());
}

#[test]
fn clip_count_and_foreign_parents_fail_atomically() {
    let mut store = Store::default();
    let mut other = Store::default();
    let geometry = store.create_geometry(super::super::tests::mesh()).unwrap();
    let foreign_geometry = other.create_geometry(super::super::tests::mesh()).unwrap();
    let foreign = other
        .create_clip(clip(foreign_geometry, None), DrawingPermissions::ALL)
        .unwrap();
    assert_eq!(
        store
            .create_clip(clip(geometry, Some(foreign)), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::StaleHandle
    );
    for _ in 0..mod_api::drawing::MAX_DRAW_CLIPS {
        store
            .create_clip(clip(geometry, None), DrawingPermissions::ALL)
            .unwrap();
    }
    assert_eq!(
        store
            .create_clip(clip(geometry, None), DrawingPermissions::ALL)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
    store.commit();
    let snapshot = Arc::clone(store.snapshot());
    store.begin();
    store.commit();
    assert!(Arc::ptr_eq(&snapshot, store.snapshot()));
}

#[test]
fn transferred_clips_remap_all_references_and_reject_cycles() {
    let mut store = Store::default();
    let geometry = store.create_geometry(super::super::tests::mesh()).unwrap();
    let parent = store
        .create_clip(clip(geometry, None), DrawingPermissions::ALL)
        .unwrap();
    let child = store
        .create_clip(clip(geometry, Some(parent)), DrawingPermissions::ALL)
        .unwrap();
    let mut instance = super::super::tests::drawing(geometry);
    instance.clip = Some(child);
    store.create(instance, DrawingPermissions::ALL).unwrap();
    store.commit();
    let mut wire = transfer::Snapshot::from_resources(store.snapshot());
    let (received, _) = wire
        .admit(
            None,
            DrawingPermissions::ALL,
            1,
            0,
            |_| true,
            |id| Handle { owner: 900, ..id },
        )
        .unwrap();
    assert_eq!(received.clips.len(), 2);
    assert!(
        received
            .clips
            .values()
            .all(|clip| clip.geometry.owner == 900 && clip.parent.is_none_or(|id| id.owner == 900))
    );
    assert!(
        received
            .instances
            .values()
            .all(|draw| draw.clip.unwrap().owner == 900)
    );
    wire.clips[0].1.parent = Some(child);
    assert_eq!(
        wire.admit(None, DrawingPermissions::ALL, 1, 0, |_| true, |id| id)
            .unwrap_err()
            .code,
        ErrorCode::BudgetExceeded
    );
}

#[test]
fn actor_despawn_prunes_nested_clips_and_dependent_draws() {
    let mut store = Store::default();
    store.bind_session(1, 0);
    let key = EntityKey {
        session: 1,
        dimension: 0,
        runtime_id: 42,
        spawn_revision: 1,
    };
    store.reconcile_entities(vec![key], &[]).unwrap();
    let entity = store.observe_entities().unwrap()[0].0;
    let geometry = store.create_geometry(super::super::tests::mesh()).unwrap();
    let mut mask = clip(geometry, None);
    mask.anchor = Anchor::Entity(EntityAnchor {
        entity,
        body_rotation: true,
    });
    let parent = store.create_clip(mask, DrawingPermissions::ALL).unwrap();
    let child = store
        .create_clip(clip(geometry, Some(parent)), DrawingPermissions::ALL)
        .unwrap();
    let mut instance = super::super::tests::drawing(geometry);
    instance.clip = Some(child);
    store.create(instance, DrawingPermissions::ALL).unwrap();
    store.commit();
    let wire = transfer::Snapshot::from_resources(store.snapshot());
    let (received, stale) = wire
        .admit(None, DrawingPermissions::ALL, 1, 0, |_| false, |id| id)
        .unwrap();
    assert_eq!(stale, 1);
    assert!(received.clips.is_empty());
    assert!(received.instances.is_empty());
    store.reconcile_entities(vec![], &[key]).unwrap();
    assert!(store.snapshot().clips.is_empty());
    assert!(store.snapshot().instances.is_empty());
    let wire = transfer::Snapshot::from_resources(store.snapshot());
    assert!(
        wire.admit(None, DrawingPermissions::ALL, 1, 0, |_| false, |id| id)
            .is_ok()
    );
}
