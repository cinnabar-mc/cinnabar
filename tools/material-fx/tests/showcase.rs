//! The showcase builds into an artifact the Experience runtime loads and a client part bundle
//! the client verifies, both from the real guests.

use material_fx::artifact::{self, Guests, ID};
use std::path::Path;

/// The client part's materials, compiled natively so the test reads the same table the guest
/// declares from.
/// The control channel's operations, numbered from `experience.toml` by the tool's build script.
#[allow(dead_code)]
mod operations {
    include!(concat!(env!("OUT_DIR"), "/operations.rs"));
}

#[path = "../../../examples/experiences/material-fx/client/src/effects.rs"]
#[allow(dead_code)]
mod effects;

#[test]
fn the_showcase_builds_a_loadable_artifact_and_a_verified_world_material_bundle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let guests = Guests::build(&root).expect("the guests build");
    let out = tempfile::tempdir().unwrap();
    let built = artifact::build(&root, out.path(), &guests).unwrap();
    let (engine, _ticker) = experience_runtime::load::engine().unwrap();
    let loaded = experience_runtime::load::load(&engine, &built.experiences.join(ID)).unwrap();
    let mut blocks: Vec<_> = loaded
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect();
    blocks.sort_unstable();
    let mut looks: Vec<String> = artifact::BLOCK_LOOKS
        .iter()
        .map(|(name, ..)| format!("{ID}:{name}"))
        .collect();
    looks.sort_unstable();
    assert_eq!(
        blocks, looks,
        "every registered block has a Bedrock look and no other does"
    );
    every_bedrock_look_resolves(&loaded.blocks);
    glass_blocks_blend_like_their_materials(&loaded.blocks);
    let bytes = std::fs::read(&built.cxb).unwrap();
    let manifest = server_experience::bundle::offline::verify(&bytes)
        .unwrap()
        .manifest;
    assert_eq!(manifest.id, ID);
    assert_eq!(manifest.namespace.as_deref(), Some(ID));
    assert!(
        manifest
            .permissions
            .contains(&server_experience::manifest::Permission::RenderWorldMaterial)
    );
}

/// The Bedrock look paths of `block`: its cube textures, or its visual's materials.
fn look_paths(block: &experience_runtime::protocol::BlockDef) -> Vec<&str> {
    match &block.visual {
        Some(visual) => visual.materials.iter().map(|m| m.path.as_str()).collect(),
        None => block.textures.iter().map(|t| t.path.as_str()).collect(),
    }
}

/// Every block shows a Bedrock look that exists in the artifact and decodes as a 16×16 image,
/// so a block whose material is revoked or still compiling never draws the missing texture.
fn every_bedrock_look_resolves(blocks: &[experience_runtime::protocol::BlockDef]) {
    for block in blocks {
        let paths = look_paths(block);
        assert!(!paths.is_empty(), "{} has no Bedrock look", block.id);
        for path in paths {
            let image = image::open(path)
                .unwrap_or_else(|error| panic!("{}'s look {path}: {error}", block.id));
            assert_eq!(
                (image.width(), image.height()),
                (16, 16),
                "{}'s look {path}",
                block.id
            );
        }
    }
}

/// A block whose world material lets the world behind it show has a Bedrock look that blends
/// too, so neither hides its neighbours' faces; every other block is an opaque cube.
fn glass_blocks_blend_like_their_materials(blocks: &[experience_runtime::protocol::BlockDef]) {
    use experience_runtime::protocol::RenderMethod;
    use experience_sdk::client::world_material::{Blend, Target};
    for effect in &effects::EFFECTS {
        let Some(Target::Block(id)) = (effect.target)() else {
            continue;
        };
        let block = blocks.iter().find(|block| block.id == id).unwrap();
        let blends = block.visual.as_ref().is_some_and(|visual| {
            visual
                .materials
                .iter()
                .all(|material| material.render_method == RenderMethod::Blend)
        });
        assert_eq!(blends, effect.blend != Blend::Opaque, "{id}");
    }
}

/// The portal's turn and the heartbeat, as the shaders compute them from material `id`'s
/// parameters `rate` and `offset` at session second `seconds`.
fn phase_of(state: &effects::State, id: u32, (rate, offset): (usize, usize), seconds: f64) -> f64 {
    let params = effects::spec(id, state).params;
    seconds * f64::from(params[rate]) + f64::from(params[offset])
}

#[test]
fn server_updates_change_how_fast_the_portal_and_heartbeat_go_but_never_jump_them() {
    let mut state = effects::State::default();
    // The player walks towards the portal a quarter block at a time, late in the session. The
    // turn repeats every portal cycle, which its offset is kept within.
    let spin = (8, 9);
    let cycle = f64::from(effects::PORTAL_CYCLE);
    for (step, distance) in (0..8).zip((0..).map(|step| 9_000 - step * 250)) {
        let at_ms = 2_000_000 + step * 700;
        let at = at_ms as f64 / 1000.0;
        let before = phase_of(&state, effects::PORTAL, spin, at);
        state.apply(11, [distance, at_ms, 0, 0]);
        let after = phase_of(&state, effects::PORTAL, spin, at);
        let gap = (after - before).rem_euclid(cycle);
        assert!(
            gap.min(cycle - gap) < 0.01,
            "step {step}: {before} then {after}"
        );
    }
    // The boss loses health while its heart beats; the beat's phase (in beats, modulo one)
    // carries on.
    let beat = (4, 7);
    state.apply(4, [900, 3_000_000, 0, 0]);
    for (step, health) in [(1, 700), (2, 400), (3, 100)] {
        let at_ms = 3_000_000 + step * 1_300;
        let at = at_ms as f64 / 1000.0;
        let before = phase_of(&state, effects::ARENA, beat, at).rem_euclid(1.0);
        state.apply(4, [health, at_ms, 0, 0]);
        let after = phase_of(&state, effects::ARENA, beat, at).rem_euclid(1.0);
        let gap = (after - before).abs();
        assert!(
            gap.min(1.0 - gap) < 0.01,
            "health {health}: {before} then {after}"
        );
    }
}

/// The host's form of a client part effects declaration.
fn host_effects(
    effects: &experience_sdk::client::world_material_effects::Effects,
) -> server_experience::world_material::WorldEffects {
    use server_experience::world_material::{WorldBloom, WorldOutline, WorldParticles};
    server_experience::world_material::WorldEffects {
        displacement_blocks: effects.displacement_blocks,
        scene: effects.scene,
        outline: effects.outline.map(|outline| WorldOutline {
            width_pixels: outline.width_pixels,
        }),
        bloom: effects.bloom.map(|bloom| WorldBloom {
            strength: bloom.strength,
            radius_pixels: bloom.radius_pixels,
        }),
        particles: effects.particles.map(|particles| WorldParticles {
            count: particles.count,
            radius_blocks: particles.radius_blocks,
        }),
        cells: effects.cells,
    }
}

/// The host's form of a client part material declaration with `effects`.
fn host_spec(
    spec: &experience_sdk::client::world_material::MaterialSpec,
    effects: server_experience::world_material::WorldEffects,
) -> server_experience::world_material::WorldMaterialSpec {
    use experience_sdk::client::world_material::{Blend, Depth};
    use server_experience::world_material::{WorldBlend, WorldDepth};
    server_experience::world_material::WorldMaterialSpec {
        source: spec.source.clone(),
        textures: spec.textures.clone(),
        params: spec.params.clone(),
        depth: match spec.depth {
            Depth::Test => WorldDepth::Test,
            Depth::Always => WorldDepth::Always,
        },
        blend: match spec.blend {
            Blend::Opaque => WorldBlend::Opaque,
            Blend::Cutout => WorldBlend::Cutout,
            Blend::Premultiplied => WorldBlend::Premultiplied,
            Blend::Additive => WorldBlend::Additive,
        },
        effects,
    }
}

/// The showcase's per-frame work budget for its effects, well inside the host's caps so the
/// whole wing costs a small, fixed share of a frame: compute cells stepped, and particles
/// drawn with each material bound to at most `PARTICLE_TARGETS` blocks or entities.
const BUDGET_CELLS_PER_FRAME: u32 = 8_192;
const BUDGET_PARTICLES_PER_FRAME: u32 = 4_096;
const PARTICLE_TARGETS: u32 = 1;

#[test]
fn every_rich_form_is_granted_with_its_permissions_refused_without_and_fits_the_frame_budget() {
    use server_experience::manifest::{Permission, WORLD_MATERIAL_EXTENSIONS};
    use server_experience::world_material::WorldAuthority;
    let assets: std::collections::BTreeSet<String> = material_fx::artifact::client_textures()
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let all: std::collections::BTreeSet<Permission> = [Permission::RenderWorldMaterial]
        .into_iter()
        .chain(WORLD_MATERIAL_EXTENSIONS.iter().map(|e| e.permission))
        .collect();
    let plain = std::collections::BTreeSet::from([Permission::RenderWorldMaterial]);
    let authority = |permissions| WorldAuthority {
        permissions,
        namespace: Some(ID),
        assets: &assets,
    };
    for state in [effects::State::default(), {
        let mut off = effects::State::default();
        off.outline = false;
        off.bloom = false;
        off
    }] {
        let mut particles = 0;
        for id in 0..effects::EFFECTS.len() as u32 {
            let plain_spec = host_spec(&effects::spec(id, &state), Default::default());
            authority(&plain)
                .validate_material(&plain_spec)
                .unwrap_or_else(|error| panic!("material {id}'s plain form: {error}"));
            let Some((spec, wanted)) = effects::rich_spec(id, &state) else {
                continue;
            };
            let rich = host_spec(&spec, host_effects(&wanted));
            authority(&all)
                .validate_material(&rich)
                .unwrap_or_else(|error| panic!("material {id}'s rich form: {error}"));
            if !rich.effects.is_none() {
                assert!(
                    authority(&plain).validate_material(&rich).is_err(),
                    "material {id}'s effects need their permissions"
                );
            }
            particles += rich.effects.particles.map_or(0, |p| p.count) * PARTICLE_TARGETS;
        }
        let cells: u32 = effects::COMPUTES
            .iter()
            .map(|&id| {
                let spec = effects::compute(id, &state);
                let host = server_experience::world_material::WorldComputeSpec {
                    source: spec.source,
                    cells: spec.cells,
                    params: spec.params,
                };
                authority(&all).validate_compute(&host).unwrap();
                assert!(authority(&plain).validate_compute(&host).is_err());
                host.cells
            })
            .sum();
        assert!(cells <= BUDGET_CELLS_PER_FRAME, "{cells} cells a frame");
        assert!(
            particles <= BUDGET_PARTICLES_PER_FRAME,
            "{particles} particles a frame"
        );
    }
}
