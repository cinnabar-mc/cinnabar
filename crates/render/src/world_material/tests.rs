//! The shared world material resources and the modules both renderers compose.

use super::*;
use mod_render::world_material::compose;

/// A scene publishing one plain material with a texture, bound to one entity.
fn scene() -> WorldMaterialScene {
    let mut scene = WorldMaterialScene::with_bound_entities(&["example:moth"]);
    let set = std::sync::Arc::make_mut(&mut scene.set);
    set.materials[0].as_mut().unwrap().textures =
        std::sync::Arc::from([abi::publication::Texture {
            width: 2,
            height: 2,
            rgba: std::sync::Arc::from([255_u8; 16]),
        }]);
    scene
}

/// The production actor shader with its instance and vertex strides, then `library`.
fn actor_with(library: &str) -> String {
    format!(
        "{}\n{library}",
        include_str!("../actor.wesl")
            .replace(
                "ACTOR_GPU_INSTANCE_WORDS",
                &format!("{}u", render_model::ACTOR_GPU_INSTANCE_WORDS),
            )
            .replace(
                "ACTOR_RIG_VERTEX_WORDS",
                &format!("{}u", render_model::ACTOR_RIG_VERTEX_WORDS),
            )
    )
}

/// The production cube and model terrain shaders, each with `library`.
fn terrain_with(library: &str) -> [String; 2] {
    [include_str!("../chunk.wesl"), include_str!("../model.wesl")]
        .map(|source| crate::material_shader::source(&format!("{source}\n{library}")))
}

#[test]
fn revocation_retains_no_gpu_resources() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<WorldMaterialGpu>();
    world.insert_resource(scene());
    world.run_system_once(prepare).unwrap();
    let gpu = world.resource::<WorldMaterialGpu>();
    assert!(gpu.has_materials() && gpu.slot(0).is_some());
    world.insert_resource(WorldMaterialScene::default());
    world.run_system_once(prepare).unwrap();
    let gpu = world.resource::<WorldMaterialGpu>();
    assert!(!gpu.has_materials() && gpu.slot(0).is_none());
}

/// Views make effects targets only while a material blooms or outlines, sized to the widest
/// of them.
#[test]
fn the_effects_pass_reaches_as_far_as_its_widest_material() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<WorldMaterialGpu>();
    let mut scene =
        WorldMaterialScene::with_bound_entities(&["example:a", "example:b", "example:c"]);
    let set = std::sync::Arc::make_mut(&mut scene.set);
    set.materials[0].as_mut().unwrap().effects.outline_pixels = Some(3.0);
    set.materials[1].as_mut().unwrap().effects.bloom = Some(abi::publication::Bloom {
        strength: 2.0,
        radius_pixels: 9.0,
    });
    world.insert_resource(scene);
    world.run_system_once(prepare).unwrap();
    let gpu = world.resource::<WorldMaterialGpu>();
    assert_eq!(gpu.effects_reach(), Some([9.0, 3.0]));
    assert!(gpu.variant(0).unwrap().effects_pass && !gpu.variant(2).unwrap().effects_pass);
    // Changed effects always come with a new key.
    let mut plain = WorldMaterialScene::with_bound_entities(&["example:a"]);
    let set = std::sync::Arc::make_mut(&mut plain.set);
    set.revision = 2;
    set.materials[0].as_mut().unwrap().key = 99;
    world.insert_resource(plain);
    world.run_system_once(prepare).unwrap();
    assert_eq!(world.resource::<WorldMaterialGpu>().effects_reach(), None);
}

/// A material reading cells binds its compute's current cells once they exist, and a new
/// compute in its place rebuilds its group.
#[test]
fn materials_reading_cells_bind_their_computes_cells() {
    use bevy::ecs::system::RunSystemOnce;
    let compute = |key| abi::publication::PublishedCompute {
        key,
        compute: std::sync::Arc::new(
            abi::compute::compose(
                "fn step(input: WorldCellInput) -> vec4<f32> { return cell(input.index); }",
            )
            .unwrap(),
        ),
        cells: 64,
        params: [0.0; 16],
    };
    let reading = |computes: Vec<abi::publication::PublishedCompute>, revision| {
        let mut scene = WorldMaterialScene::with_bound_entities(&["example:moth"]);
        let set = std::sync::Arc::make_mut(&mut scene.set);
        set.revision = revision;
        let material = set.materials[0].as_mut().unwrap();
        material.material = std::sync::Arc::new(
            abi::compose_with(
                "fn shade(input: WorldMaterialInput) -> Surface { return Surface(cell(0u).rgb, 1.0, vec3<f32>(0.0)); }",
                0,
                abi::Features {
                    cells: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        material.effects.cells = (!computes.is_empty()).then_some(0);
        set.computes = computes;
        scene
    };
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<WorldMaterialGpu>();
    // Without its compute the material waits.
    world.insert_resource(reading(Vec::new(), 2));
    world.run_system_once(prepare).unwrap();
    assert!(world.resource::<WorldMaterialGpu>().slot(0).is_none());
    world.insert_resource(reading(vec![compute(90)], 3));
    world.run_system_once(prepare).unwrap();
    let gpu = world.resource::<WorldMaterialGpu>();
    assert!(gpu.slot(0).is_some());
    assert_eq!(gpu.computes.get(0).map(|(key, _)| key), Some(90));
    assert_eq!(gpu.entries.values().next().unwrap().cells, Some(90));
    world.insert_resource(reading(vec![compute(91)], 4));
    world.run_system_once(prepare).unwrap();
    let gpu = world.resource::<WorldMaterialGpu>();
    assert_eq!(
        gpu.computes.entries.len(),
        1,
        "the replaced compute's cells are freed"
    );
    assert_eq!(gpu.entries.values().next().unwrap().cells, Some(91));
}

/// The production actor and cube modules compose and validate with a world material library
/// in every blend class, as Bevy's WESL composition builds them.
#[test]
fn world_material_modules_compose_for_actors_and_terrain() {
    let material = compose(
        "fn shade(input: WorldMaterialInput) -> Surface {
            let glow = texture(1u, input.uv).rgb * param(input, 3u);
            return Surface(texture(0u, input.uv).rgb * input.light.rgb, input.base.a, glow);
        }",
        2,
    )
    .unwrap();
    let actor =
        actor_with(&material.library(crate::actor_render::world_materials::WORLD_MATERIAL_GROUP));
    let [terrain, model] =
        terrain_with(&material.library(crate::chunk::world_materials::WORLD_MATERIAL_GROUP));
    for class in [
        "WORLD_MATERIAL_OPAQUE",
        "WORLD_MATERIAL_CUTOUT",
        "WORLD_MATERIAL_PREMULTIPLY",
    ] {
        crate::shader_source::composed(&actor, &["WORLD_MATERIAL", class]);
        crate::shader_source::composed(&terrain, &["WORLD_MATERIAL", class]);
        crate::shader_source::composed(&model, &["WORLD_MATERIAL", class]);
    }
    // Translucent classes also compose for the gamma-blended translucent pass.
    for module in [&terrain, &model] {
        crate::shader_source::composed(
            module,
            &[
                "WORLD_MATERIAL",
                "WORLD_MATERIAL_PREMULTIPLY",
                "NATIVE_GAMMA_BLEND",
            ],
        );
    }
    // The Bedrock fallback passes compose without any library.
    for source in [include_str!("../chunk.wesl"), include_str!("../model.wesl")] {
        crate::shader_source::composed(
            &crate::material_shader::source(source),
            &["WORLD_MATERIAL_FALLBACK"],
        );
    }
    // So does the translucent pass that draws bound faces as Bedrock while unready.
    crate::shader_source::composed(
        include_str!("../transparent_terrain.wesl"),
        &["WORLD_MATERIAL_AS_BEDROCK"],
    );
}

/// Each effect's host code composes with a library that has its functions: displacement in
/// the vertex stages, the scene in the translucent modules, the effects pass with and without
/// an outline, and the particle stage.
#[test]
fn effect_modules_compose_for_actors_terrain_and_particles() {
    let features = |set: fn(&mut abi::Features)| {
        let mut features = abi::Features::default();
        set(&mut features);
        features
    };
    let compose_all = |source: &str, features: abi::Features, defs: &[&str]| {
        let material = mod_render::world_material::compose_with(source, 0, features).unwrap();
        crate::shader_source::composed(
            &actor_with(
                &material.library(crate::actor_render::world_materials::WORLD_MATERIAL_GROUP),
            ),
            defs,
        );
        for module in
            terrain_with(&material.library(crate::chunk::world_materials::WORLD_MATERIAL_GROUP))
        {
            crate::shader_source::composed(&module, defs);
        }
    };
    compose_all(
        "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }
fn displace(input: WorldVertexInput) -> vec3<f32> { return input.normal * input.phase; }",
        features(|f| f.displace = true),
        &[
            "WORLD_MATERIAL",
            "WORLD_MATERIAL_OPAQUE",
            "WORLD_MATERIAL_DISPLACE",
        ],
    );
    let scene = "fn shade(input: WorldMaterialInput) -> Surface {
    let soft = clamp(scene_depth(vec2<f32>(0.0)) - fragment_depth(), 0.0, 1.0);
    return Surface(scene_color(input.normal.xy * 3.0), soft, vec3<f32>(0.0));
}";
    let scene_defs = [
        "WORLD_MATERIAL",
        "WORLD_MATERIAL_PREMULTIPLY",
        "WORLD_MATERIAL_SCENE",
    ];
    compose_all(scene, features(|f| f.scene = true), &scene_defs);
    let reader =
        mod_render::world_material::compose_with(scene, 0, features(|f| f.scene = true)).unwrap();
    let mut gamma = scene_defs.to_vec();
    gamma.push("NATIVE_GAMMA_BLEND");
    for module in terrain_with(&reader.library(crate::chunk::world_materials::WORLD_MATERIAL_GROUP))
    {
        crate::shader_source::composed(&module, &gamma);
    }
    let effects = "fn shade(input: WorldMaterialInput) -> Surface { return Surface(input.base.rgb, 1.0, vec3<f32>(2.0)); }
fn outline(input: WorldMaterialInput) -> vec3<f32> { return vec3<f32>(1.0, 0.5, 0.0); }";
    let effects_defs = [
        "WORLD_MATERIAL",
        "WORLD_MATERIAL_CUTOUT",
        "WORLD_MATERIAL_EFFECTS",
    ];
    compose_all(effects, abi::Features::default(), &effects_defs);
    compose_all(effects, features(|f| f.outline = true), &effects_defs);
    // A material with particles composes with the particle stage.
    let particle = mod_render::world_material::compose_with(
        "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }
fn particle(input: WorldParticleInput) -> WorldParticle {
    return WorldParticle(vec3<f32>(f32(input.index)), 1.0, vec3<f32>(1.0), 1.0);
}",
        0,
        features(|f| f.particles = true),
    )
    .unwrap();
    let module = naga::front::wgsl::parse_str(&format!(
        "{}\n{}",
        particle.library(1),
        include_str!("particles.wgsl")
    ))
    .unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
fn one_program_registers_one_module_per_renderer_that_revocation_keeps() {
    let mut shaders = bevy::asset::Assets::<Shader>::default();
    let mut registered = BTreeSet::new();
    let families = [
        crate::actor_render::world_materials::family(),
        crate::chunk::world_materials::family(),
    ];
    let published = scene();
    register_shaders(&families, &published, &mut shaders, &mut registered);
    let variant = WorldVariant::of(published.set.published().next().unwrap()).unwrap();
    for family in &families {
        assert!(shaders.get(variant.shader(family.namespace).id()).is_some());
    }
    let modules = shaders.len();
    // The same program under a new key, as a later session declares it, reuses the module.
    let mut again = scene();
    let set = std::sync::Arc::make_mut(&mut again.set);
    set.materials[0].as_mut().unwrap().key = 99;
    register_shaders(&families, &again, &mut shaders, &mut registered);
    assert_eq!(shaders.len(), modules);
    // Revocation keeps the module, so its pipelines idle instead of waiting for it forever.
    register_shaders(
        &families,
        &WorldMaterialScene::default(),
        &mut shaders,
        &mut registered,
    );
    for family in &families {
        assert!(shaders.get(variant.shader(family.namespace).id()).is_some());
    }
    // Another program has modules of its own.
    let mut other = scene();
    let set = std::sync::Arc::make_mut(&mut other.set);
    let material = set.materials[0].as_mut().unwrap();
    material.material = std::sync::Arc::new(abi::WorldMaterial {
        source: "fn shade(input: WorldMaterialInput) -> Surface { return Surface(vec3<f32>(1.0), 1.0, vec3<f32>(0.0)); }".into(),
        textures: 1,
        features: Default::default(),
    });
    let other_variant = WorldVariant::of(material).unwrap();
    assert_ne!(other_variant.program, variant.program);
    register_shaders(&families, &other, &mut shaders, &mut registered);
    assert_eq!(shaders.len(), modules + families.len());
}

#[test]
fn pipeline_ids_of_programs_no_longer_published_are_pruned() {
    use bevy::ecs::system::RunSystemOnce;
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<WorldMaterialGpu>();
    world.insert_resource(scene());
    world.run_system_once(prepare).unwrap();
    let live = world
        .resource::<WorldMaterialGpu>()
        .variant(0)
        .unwrap()
        .program;
    let mut ids = std::collections::HashMap::from([(live, 1_u32), (live ^ 1, 2)]);
    let mut pruned = None;
    prune_pipelines(
        &mut ids,
        &mut pruned,
        world.resource::<WorldMaterialGpu>(),
        |program| *program,
    );
    assert_eq!(ids.keys().copied().collect::<Vec<_>>(), [live]);
    // Revocation leaves nothing to scan.
    world.insert_resource(WorldMaterialScene::default());
    world.run_system_once(prepare).unwrap();
    prune_pipelines(
        &mut ids,
        &mut pruned,
        world.resource::<WorldMaterialGpu>(),
        |program| *program,
    );
    assert!(ids.is_empty());
}

#[test]
fn a_module_still_loading_is_not_a_lasting_failure() {
    use bevy::render::render_resource::CachedPipelineState;
    use bevy::shader::ShaderCacheError;
    let loading = CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(
        shader(TERRAIN_SHADERS, 1).id(),
    ));
    assert!(lasting_error(&loading).is_none());
    let import = CachedPipelineState::Err(ShaderCacheError::ShaderImportNotYetAvailable);
    assert!(lasting_error(&import).is_none());
    let broken = CachedPipelineState::Err(ShaderCacheError::CreateShaderModule("invalid".into()));
    assert!(lasting_error(&broken).is_some());
    assert!(lasting_error(&CachedPipelineState::Queued).is_none());
}

/// The phase follows the wall clock, so a server timing an effect by Unix time agrees.
#[test]
fn the_phase_is_unix_time_modulo_the_wrap() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = bevy::prelude::World::new();
    world.init_resource::<WorldMaterialPhase>();
    let at = std::time::Duration::from_millis(1_790_000_000_250);
    world
        .run_system_once(move |phase: ResMut<WorldMaterialPhase>| set_phase(phase, at))
        .unwrap();
    assert_eq!(
        world.resource::<WorldMaterialPhase>().0,
        mod_render::material::phase(at.as_secs_f64())
    );
}
