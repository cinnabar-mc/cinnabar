use super::super::*;
use assets::{CompiledEntityAssets, CompiledMolangExpression, MolangSymbol, MolangSymbolKind};
use protocol_ids::ActorDataKey;

/// Appends one original scalar program to the fixture's expression table.
fn expression(compiled: &mut CompiledEntityAssets, program: Vec<MolangOp>) -> u32 {
    let mut expressions = compiled.molang_expressions.to_vec();
    let mut ops = compiled.molang_ops.to_vec();
    let index = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: program.len() as u16,
        max_stack: assets::molang_program_stack(&program).unwrap(),
    });
    ops.extend(program);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    index
}

/// Two player controllers select arbitrary metadata ranges; held visibility is independent.
fn fixture(frame_gate: bool) -> CompiledEntityAssets {
    use MolangOp::*;
    let scalar = crate::test_support::scalar;
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = None;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.molang_ops = Box::new([]);
    compiled.molang_expressions = Box::new([]);
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.frame_alpha"),
        (MolangSymbolKind::Query, "query.is_in_ui"),
        (MolangSymbolKind::Query, "query.is_local_player"),
        (MolangSymbolKind::Query, "query.mark_variant"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    let mut first = vec![
        LoadQuery(4),
        Push(scalar(41.0)),
        GreaterEqual,
        LoadQuery(3),
        Multiply,
    ];
    if frame_gate {
        first.extend([LoadQuery(1), Push(scalar(0.5)), Greater, Multiply]);
    }
    // The final multiply leaves the earlier predicate under the frame comparison.
    let first = expression(&mut compiled, first);
    if frame_gate {
        compiled.molang_expressions[first as usize].max_stack = 3;
    }
    let second = expression(
        &mut compiled,
        vec![
            LoadQuery(4),
            Push(scalar(42.0)),
            Equal,
            LoadQuery(3),
            Multiply,
        ],
    );
    let hide = expression(
        &mut compiled,
        vec![
            LoadQuery(4),
            Push(scalar(37.0)),
            Equal,
            LoadQuery(3),
            Multiply,
            LoadQuery(2),
            Not,
            Multiply,
        ],
    );
    compiled.rig_bindings[0].hide_held_items = Some(hide);
    let mut layer = compiled.render.layers[0];
    layer.condition = Some(first);
    let mut second_layer = layer;
    second_layer.condition = Some(second);
    second_layer.first_slot = 1;
    let mut slot = compiled.render.slots[0];
    slot.first_candidate = 1;
    compiled.render.slots = vec![compiled.render.slots[0], slot].into();
    compiled.render.candidates =
        vec![compiled.render.candidates[0], compiled.render.candidates[0]].into();
    compiled.render.layers = vec![layer, second_layer].into();
    compiled
}

/// Installs a player without requiring any external pack or account.
fn store(compiled: CompiledEntityAssets) -> (ActorAnimationStore, TestActor) {
    let mut actor = crate::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Fixture".into(),
    };
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    ));
    store.insert(1, 0, &actor);
    (store, actor)
}

/// Replaces the actor's metadata through the same query input used by all render expressions.
fn mark(actor: &mut TestActor, value: i32) {
    actor.metadata.insert(
        ActorDataKey::MarkVariant.wire_value(),
        ActorMetadataValue::Int(value),
    );
}

/// Completes world and independent UI evaluations with explicit local identity.
fn advance(store: &mut ActorAnimationStore, actor: &TestActor, local: bool) {
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        local.then_some(actor.runtime_id),
        true,
        true,
        |_| ActorTickContext {
            is_local: local,
            is_local_player: local,
            ..Default::default()
        },
    );
}

#[test]
fn visibility_distinguishes_all_false_one_true_and_multiple_true_controllers() {
    for local in [false, true] {
        let (mut store, mut actor) = store(fixture(false));
        for (value, count) in [(37, 0), (41, 1), (42, 2), (11, 0), (41, 1)] {
            mark(&mut actor, value);
            advance(&mut store, &actor, local);
            let rig = store.get(actor.runtime_id).unwrap();
            let count = if local { count } else { 0 };
            assert_eq!(rig.render.len(), count);
            assert_eq!(
                rig.visibility.controllers,
                if count == 0 {
                    ControllerActivity::Inactive
                } else {
                    ControllerActivity::Active
                }
            );
            assert_eq!(rig.visibility.hides_held_items(), local && value == 37);
        }
    }
}

#[test]
fn visibility_keeps_no_controllers_unresolved_artwork_and_unevaluated_distinct() {
    for no_controllers in [false, true] {
        let mut compiled = fixture(false);
        if no_controllers {
            compiled.render = Default::default();
        } else {
            for layer in &mut compiled.render.layers {
                layer.unresolved_textures = true;
                layer.first_slot = 0;
                layer.slot_count = 0;
            }
            compiled.render.slots = Box::new([]);
            compiled.render.candidates = Box::new([]);
        }
        let (mut store, mut actor) = store(compiled);
        assert_eq!(
            store.get(1).unwrap().visibility.controllers,
            ControllerActivity::NotEvaluated
        );
        mark(&mut actor, 41);
        advance(&mut store, &actor, true);
        let rig = store.get(1).unwrap();
        assert!(rig.render.is_empty());
        assert_eq!(
            rig.visibility.controllers,
            if no_controllers {
                ControllerActivity::NoControllers
            } else {
                ControllerActivity::Active
            }
        );
        assert!(!rig.visibility.hides_body());
    }
}

#[test]
fn visibility_hides_world_items_but_evaluates_ui_context_independently() {
    let (mut store, mut actor) = store(fixture(false));
    for value in [37, 41, 37] {
        mark(&mut actor, value);
        advance(&mut store, &actor, true);
        assert_eq!(
            store.get(1).unwrap().visibility.hides_held_items(),
            value == 37
        );
        assert!(!store.ui_layers(1).unwrap().visibility.hides_held_items());
        assert_eq!(
            store
                .paper_doll_layers(1)
                .unwrap()
                .visibility
                .hides_held_items(),
            value == 37
        );
        assert_eq!(
            store.ui_layers(1).unwrap().visibility.controllers,
            store.get(1).unwrap().visibility.controllers
        );
    }
}

#[test]
fn visibility_retained_samples_follow_frame_metadata_and_reset() {
    let (mut store, mut actor) = store(fixture(true));
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    for (value, alpha, body, held) in [
        (41, 0.25, true, false),
        (41, 0.75, false, false),
        (37, 0.75, true, true),
        (37, 0.75, true, true),
        (41, 0.75, false, false),
        (41, 0.25, true, false),
    ] {
        mark(&mut actor, value);
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let output = store
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap();
        assert_eq!(output.visibility.hides_body(), body, "{value}, {alpha}");
        assert_eq!(
            output.visibility.hides_held_items(),
            held,
            "{value}, {alpha}"
        );
    }
    mark(&mut actor, 37);
    store.mark_reset(1);
    advance(&mut store, &actor, true);
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    assert!(
        store
            .render_layers(&actor, 0.75, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap()
            .visibility
            .hides_held_items()
    );
}

#[test]
fn visibility_budget_fallback_retains_completed_policy() {
    let (mut store, mut actor) = store(fixture(true));
    mark(&mut actor, 37);
    advance(&mut store, &actor, true);
    let completed = store.get(1).unwrap().visibility;
    mark(&mut actor, 42);
    for mut budget in [0, 1] {
        let output = store
            .render_layers(&actor, 0.75, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap();
        assert_eq!(output.visibility, completed);
    }
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    assert!(
        !store
            .render_layers(&actor, 0.75, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap()
            .visibility
            .hides_body()
    );
}

#[test]
fn visibility_held_property_alone_samples_frames_and_culled_owners() {
    let mut compiled = fixture(false);
    let hide = expression(
        &mut compiled,
        vec![
            MolangOp::LoadQuery(1),
            MolangOp::Push(crate::test_support::scalar(0.5)),
            MolangOp::Greater,
        ],
    );
    compiled.rig_bindings[0].hide_held_items = Some(hide);
    let (mut store, mut actor) = store(compiled);
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    store.rigs.values_mut().next().unwrap().culled = true;
    let actors = HashMap::from([(actor.runtime_id, actor)]);
    let tick = store.get(1).unwrap();
    assert!(!tick.visibility.hides_held_items());
    for (alpha, hidden) in [(0.25, false), (0.75, true), (0.75, true), (0.25, false)] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
        let sampled = frame.sample_equipment_properties(tick);
        assert_eq!(sampled.visibility.hides_held_items(), hidden);
        assert!(!sampled.visibility.hides_body());
        assert_eq!(frame.visibility(1), Some(sampled.visibility));
    }
    assert_eq!(store.get(1).unwrap().visibility, tick.visibility);
}

#[test]
fn visibility_held_property_alone_samples_camera_at_zero_partial_tick() {
    let mut compiled = fixture(false);
    compiled.molang_symbols[1].identifier = "query.distance_from_camera".into();
    let hide = expression(
        &mut compiled,
        vec![
            MolangOp::LoadQuery(1),
            MolangOp::Push(crate::test_support::scalar(8.0)),
            MolangOp::Greater,
        ],
    );
    compiled.rig_bindings[0].hide_held_items = Some(hide);
    let (mut store, mut actor) = store(compiled);
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    for (distance, hidden) in [(5.0, false), (9.0, true), (5.0, false)] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let output = store
            .render_layers(
                &actor,
                0.0,
                [0.0; 2],
                [0.0, 0.0, distance],
                &mut budget,
                true,
            )
            .unwrap();
        assert_eq!(output.visibility.hides_held_items(), hidden);
    }
}

#[test]
fn visibility_active_controller_with_unavailable_geometry_keeps_body_fallback() {
    let mut compiled = fixture(false);
    let mut geometry = compiled.geometries[0].clone();
    geometry.identifier = "geometry.layer".into();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        2,
        assets::EntityAssetSymbol {
            kind: assets::EntityAssetKind::Geometry,
            identifier: geometry.identifier.clone(),
            source_index: geometry.source_index,
            dependencies: Box::new([]),
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
        },
    );
    compiled.symbols = symbols.into();
    compiled.geometries = vec![compiled.geometries[0].clone(), geometry].into();
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    compiled.animation_clips[0].symbol += 1;
    compiled.render.layers[0].geometry_count = 1;
    compiled.render.layers[1].first_geometry = 1;
    compiled.render.geometries = vec![assets::EntityRenderGeometry {
        condition: None,
        geometry: 1,
    }]
    .into();
    let (mut store, mut actor) = store(compiled);
    store
        .rigs
        .values_mut()
        .next()
        .unwrap()
        .layer_skeletons
        .insert(1, None);
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    let rig = store.get(1).unwrap();
    assert!(rig.render.is_empty());
    assert_eq!(rig.visibility.controllers, ControllerActivity::Active);
    assert!(!rig.visibility.hides_body());
}

#[test]
fn visibility_ui_budget_fallback_never_inherits_world_policy() {
    for limit in 0..48 {
        let (mut store, mut actor) = store(fixture(false));
        mark(&mut actor, 37);
        advance(&mut store, &actor, true);
        let state = store.rigs.values_mut().next().unwrap();
        let world = state.visibility;
        let ui = state.ui_animation.as_ref().unwrap().visibility;
        assert!(world.hides_held_items());
        assert!(!ui.hides_held_items());
        let mut remaining = limit;
        let mut budget = EvalBudget {
            actor_left: limit,
            world_left: &mut remaining,
            work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_FRAME,
            transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
            used: 0,
            stack: Default::default(),
            static_draw: None,
        };
        hud::evaluate(
            store.assets.as_deref().unwrap(),
            &store.layout,
            state,
            &actor,
            &ActorTickContext {
                is_local: true,
                is_local_player: true,
                is_in_ui: true,
                ..Default::default()
            },
            store.completed_tick,
            &mut budget,
            false,
        );
        assert_eq!(state.visibility, world, "world at budget {limit}");
        assert_eq!(
            state.ui_animation.as_ref().unwrap().visibility,
            ui,
            "UI at budget {limit}"
        );
    }
}

#[test]
fn advancing_visibility_frames_reuse_owned_and_retained_layer_storage() {
    let mut compiled = fixture(false);
    compiled.rig_bindings[0].hide_held_items = Some(expression(
        &mut compiled,
        vec![
            MolangOp::LoadQuery(1),
            MolangOp::Push(crate::test_support::scalar(0.5)),
            MolangOp::Greater,
        ],
    ));
    let (mut store, mut actor) = store(compiled);
    mark(&mut actor, 41);
    advance(&mut store, &actor, true);
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    for visibility_only in [false, true] {
        let mut storage = Vec::new();
        for frame in 0..24 {
            // Repeat each alpha to exercise both fresh and retained samples; camera always advances.
            let alpha = if (frame / 2) % 2 == 0 { 0.25 } else { 0.75 };
            let allocations = test_alloc::thread_allocations();
            let output = ActorRenderFrame::new(
                &store,
                &actors,
                [frame as f32, 0.0],
                [frame as f32, 64.0, 0.0],
                alpha,
            )
            .into_layers_reusing(actor.runtime_id, false, visibility_only, &mut storage)
            .unwrap();
            assert!(!output.render.is_empty());
            assert_eq!(output.visibility.hides_held_items(), alpha > 0.5);
            if let std::borrow::Cow::Owned(layers) = output.render {
                storage = layers;
            }
            if frame >= 4 {
                assert_eq!(
                    test_alloc::thread_allocations(),
                    allocations,
                    "frame {frame}, visibility only {visibility_only}"
                );
            }
        }
    }
}

/// Camera and swing drive poses, while pre-animation and engine variables drive visibility.
fn variable_fixture() -> CompiledEntityAssets {
    use MolangOp::*;
    let scalar = crate::test_support::scalar;
    let mut compiled = fixture(false);
    compiled.molang_symbols[1].identifier = "query.distance_from_camera".into();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.extend(
        [
            "variable.attack_time",
            "variable.first_person_item_rotation_factor",
            "variable.first_person_rotation_factor",
            "variable.held_visibility",
            "variable.is_first_person",
        ]
        .map(|identifier| MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: identifier.into(),
        }),
    );
    compiled.molang_symbols = symbols.into();
    let held = expression(
        &mut compiled,
        vec![LoadVariable(5), StoreVariable(8), Push(scalar(1.0))],
    );
    compiled.rig_bindings[0].held_item_scale = Some(held);
    // The loop keeps setup on the generic VM, whose loop counters must not allocate per frame.
    let pre = expression(
        &mut compiled,
        vec![
            Push(scalar(2.0)),
            LoopStart(7),
            LoadQuery(1),
            Push(scalar(8.0)),
            Less,
            StoreVariable(9),
            LoopNext(2),
            LoadVariable(8),
            StoreVariable(7),
            Push(scalar(0.0)),
        ],
    );
    compiled.rig_bindings[0].pre_animation = Some(pre);
    let gate = expression(&mut compiled, vec![LoadVariable(9), Not]);
    for layer in &mut compiled.render.layers {
        layer.condition = Some(gate);
    }
    let hide = expression(
        &mut compiled,
        vec![LoadVariable(6), Push(scalar(0.5)), Greater],
    );
    compiled.rig_bindings[0].hide_held_items = Some(hide);
    let camera_pose = expression(&mut compiled, vec![LoadQuery(1)]);
    let swing_pose = expression(&mut compiled, vec![LoadVariable(5)]);
    compiled.animation_keyframes[0].expressions = [Some(camera_pose), Some(swing_pose), None];
    compiled
}

#[test]
fn variable_visibility_replays_setup_without_pose_work_or_warm_allocations() {
    let (mut store, actor) = store(variable_fixture());
    let progress = LocalSwingProgress {
        bedrock: [0.1, 0.9],
        java: [0.1, 0.9],
        frame_alpha: None,
    };
    store.sync_local_swing(actor.runtime_id, progress);
    advance(&mut store, &actor, true);
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut storage = Vec::new();
    for frame in 0..24 {
        if frame == 12 {
            advance(&mut store, &actor, true);
        }
        let alpha = if frame % 2 == 0 { 0.25 } else { 0.75 };
        let distance = if frame % 3 == 0 { 5.0 } else { 9.0 };
        let camera_rotation = [frame as f32, frame as f32 * 2.0];
        let camera_position = [
            actor.position[0],
            actor.position[1],
            actor.position[2] + distance,
        ];
        pose::work::take();
        let allocations = test_alloc::thread_allocations();
        let visibility =
            ActorRenderFrame::new(&store, &actors, camera_rotation, camera_position, alpha)
                .into_layers_reusing(actor.runtime_id, false, true, &mut storage)
                .unwrap()
                .visibility;
        let allocated = test_alloc::thread_allocations() - allocations;
        assert_eq!(pose::work::take(), (0, 0), "visibility frame {frame}");
        if frame >= 4 {
            assert_eq!(allocated, 0, "visibility frame {frame}");
        }
        assert_eq!(visibility.hides_body(), distance < 8.0);
        assert_eq!(visibility.hides_held_items(), alpha > 0.5);
        let full = ActorRenderFrame::new(&store, &actors, camera_rotation, camera_position, alpha)
            .into_layers_reusing(actor.runtime_id, false, false, &mut storage)
            .unwrap();
        assert_eq!(visibility, full.visibility, "frame {frame}");
        let (samples, compositions) = pose::work::take();
        assert!(samples > 0, "control must exercise pose sampling");
        assert!(compositions > 0, "control must exercise composition");
    }
}

#[test]
fn variable_visibility_retains_downstream_animation_and_render_writes() {
    use MolangOp::*;
    for render_write in [false, true] {
        let mut compiled = variable_fixture();
        let write = expression(
            &mut compiled,
            vec![
                LoadVariable(5),
                Push(crate::test_support::scalar(0.5)),
                Greater,
                StoreVariable(6),
                Push(crate::test_support::scalar(0.0)),
            ],
        );
        if render_write {
            compiled.render.layers[0].color = Some([write; 4]);
        } else {
            compiled.animation_keyframes[0].expressions[2] = Some(write);
        }
        let (mut store, actor) = store(compiled);
        store.sync_local_swing(
            actor.runtime_id,
            LocalSwingProgress {
                bedrock: [0.1, 0.9],
                java: [0.1, 0.9],
                frame_alpha: None,
            },
        );
        advance(&mut store, &actor, true);
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut storage = Vec::new();
        for alpha in [0.25, 0.75, 0.25] {
            let camera = [
                actor.position[0],
                actor.position[1],
                actor.position[2] + 9.0,
            ];
            let full = ActorRenderFrame::new(&store, &actors, [0.0; 2], camera, alpha)
                .into_layers_reusing(actor.runtime_id, false, false, &mut storage)
                .unwrap()
                .visibility;
            let visibility = ActorRenderFrame::new(&store, &actors, [0.0; 2], camera, alpha)
                .into_layers_reusing(actor.runtime_id, false, true, &mut storage)
                .unwrap()
                .visibility;
            assert_eq!(visibility, full);
            assert_eq!(visibility.hides_held_items(), alpha > 0.5);
        }
    }
}
