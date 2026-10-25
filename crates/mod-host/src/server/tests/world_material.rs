//! World material declarations staged by the server bundle host.

use super::*;

#[test]
fn world_declarations_are_refused_against_retained_state_and_kept_across_commits() {
    use server_experience::world_material::{
        WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
    };
    let mut state = state();
    state.capabilities.scope.permissions = BTreeSet::from([Permission::RenderWorldMaterial]);
    state.capabilities.namespace = Some("example".into());
    state.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    let material = WorldMaterialSpec {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }".into(),
        textures: Vec::new(),
        params: Vec::new(),
        depth: WorldDepth::Test,
        blend: WorldBlend::Opaque,
        effects: Default::default(),
    };
    let bind = |material| Command::WorldBinding {
        id: 0,
        binding: Some(WorldBinding {
            target: WorldTarget::Entity("example:moth".into()),
            material,
        }),
    };
    assert!(
        state.stage(bind(1)).unwrap().is_err(),
        "no such material yet"
    );
    state
        .stage(Command::WorldMaterial {
            id: 1,
            material: Some(material),
        })
        .unwrap()
        .unwrap();
    state.stage(bind(1)).unwrap().unwrap();
    // Committing keeps the declarations for later callbacks.
    state.world_undo.clear();
    state.begin_output().unwrap();
    assert!(
        state
            .stage(Command::WorldMaterial {
                id: 1,
                material: None,
            })
            .unwrap()
            .is_err(),
        "a bound material cannot be removed"
    );
    let outside = Command::WorldBinding {
        id: 2,
        binding: Some(WorldBinding {
            target: WorldTarget::Block("minecraft:stone".into()),
            material: 1,
        }),
    };
    assert!(state.stage(outside).unwrap().is_err());
    assert!(
        state.commands.is_empty(),
        "refused declarations stage nothing"
    );
}

#[test]
fn hostile_world_shaders_are_refused_when_declared_and_compiles_are_bounded() {
    use super::cinnabar::server_experience::world_material::{
        Blend, Depth, Host, MaterialSpec, Target,
    };
    let mut state = state();
    state.capabilities.scope.permissions = BTreeSet::from([Permission::RenderWorldMaterial]);
    state.capabilities.namespace = Some("example".into());
    state.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    let spec = |source: &str, depth| MaterialSpec {
        source: source.into(),
        textures: Vec::new(),
        params: Vec::new(),
        depth,
        blend: Blend::Opaque,
    };
    let plain = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }";
    let storage =
        format!("@group(1) @binding(0) var<storage, read_write> values: array<f32>; {plain}");
    assert!(
        state
            .set_material(0, spec(&storage, Depth::Test))
            .unwrap()
            .is_err(),
        "a storage write is refused"
    );
    assert!(
        state
            .set_material(0, spec(plain, Depth::Always))
            .unwrap()
            .is_err(),
        "depth: always needs render_through_walls"
    );
    assert!(state.commands.is_empty());
    for id in 1..mod_api::materials::MAX_WORLD_MATERIAL_COMPILES_PER_CALLBACK {
        state
            .set_material(id, spec(plain, Depth::Test))
            .unwrap()
            .unwrap();
    }
    let over = state.set_material(9, spec(plain, Depth::Test)).unwrap();
    assert!(over.unwrap_err().contains("compile budget"));
    state
        .bind(0, Target::Block("example:lamp".into()), 1)
        .unwrap()
        .unwrap();
    assert!(
        state
            .bind(1, Target::Entity("minecraft:player".into()), 1)
            .unwrap()
            .is_err()
    );
    // A new callback gets a fresh compile budget.
    state.world_undo.clear();
    state.begin_output().unwrap();
    state
        .set_material(9, spec(plain, Depth::Test))
        .unwrap()
        .unwrap();
}

#[test]
fn redeclaring_only_parameters_spends_no_compile() {
    use super::cinnabar::server_experience::world_material::{Blend, Depth, Host, MaterialSpec};
    let mut state = state();
    state.capabilities.scope.permissions = BTreeSet::from([Permission::RenderWorldMaterial]);
    state.capabilities.namespace = Some("example".into());
    state.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    let spec = |params: Vec<f32>| MaterialSpec {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }".into(),
        textures: Vec::new(),
        params,
        depth: Depth::Test,
        blend: Blend::Opaque,
    };
    state.set_material(0, spec(vec![0.0])).unwrap().unwrap();
    // Far more parameter updates than the per-callback compile budget allows compiles.
    for step in 0..4 * mod_api::materials::MAX_WORLD_MATERIAL_COMPILES_PER_CALLBACK {
        state
            .set_material(0, spec(vec![step as f32]))
            .unwrap()
            .unwrap();
    }
    assert_eq!(state.world_compiles, 1);
}

#[test]
fn staging_a_declaration_copies_none_of_the_others_and_a_failed_callback_undoes_it() {
    use server_experience::world_material::{
        WorldBinding, WorldBlend, WorldDepth, WorldMaterialSpec, WorldTarget,
    };
    let mut state = state();
    state.capabilities.scope.permissions = BTreeSet::from([Permission::RenderWorldMaterial]);
    state.capabilities.namespace = Some("example".into());
    state.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    let material = |id: u32| WorldMaterialSpec {
        source: format!(
            "fn shade(input: WorldMaterialInput) -> Surface {{ return Surface(vec3<f32>({id}.0), 1.0, vec3<f32>(0.0)); }}"
        ),
        textures: Vec::new(),
        params: vec![0.0; 16],
        depth: WorldDepth::Test,
        blend: WorldBlend::Opaque,
        effects: Default::default(),
    };
    for id in 0..200 {
        state
            .stage_world(&Command::WorldMaterial {
                id,
                material: Some(material(id)),
            })
            .unwrap();
    }
    state.world_undo.clear();
    let committed = state.world.clone();
    let bind = Command::WorldBinding {
        id: 0,
        binding: Some(WorldBinding {
            target: WorldTarget::Block("example:lamp".into()),
            material: 7,
        }),
    };
    let (staged, allocations) = test_alloc::count(|| state.stage_world(&bind));
    staged.unwrap();
    assert!(
        allocations < 16,
        "staging one binding made {allocations} allocations"
    );
    // A failed callback's next output starts from what was committed.
    state.begin_output().unwrap();
    assert_eq!(state.world, committed);
}
