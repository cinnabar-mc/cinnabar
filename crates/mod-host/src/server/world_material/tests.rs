//! The `world-material-effects` import: each effect stages only under its own permission,
//! its functions are validated with the material, and compute declarations are bounded.

use super::super::tests::state;
use super::*;
use crate::server::cinnabar::server_experience::{
    world_material::{Blend, Depth, MaterialSpec},
    world_material_effects::{ComputeSpec, Effects, Host, Outline},
};
use server_experience::manifest::Permission;
use std::collections::BTreeSet;

/// A state granted `extra` besides `render_world_material`.
fn granted(extra: &[Permission]) -> State {
    let mut state = state();
    state.capabilities.scope.permissions = BTreeSet::from([Permission::RenderWorldMaterial]);
    state.capabilities.scope.permissions.extend(extra);
    state.capabilities.namespace = Some("example".into());
    state.capabilities.scope.memory_bytes = server_experience::policy::MAX_GUEST_MEMORY;
    state
}

fn spec(source: &str) -> MaterialSpec {
    MaterialSpec {
        source: source.into(),
        textures: Vec::new(),
        params: vec![0.5],
        depth: Depth::Test,
        blend: Blend::Premultiplied,
    }
}

const WAVE: &str = "fn shade(input: WorldMaterialInput) -> Surface { return surface(input); }
fn displace(input: WorldVertexInput) -> vec3<f32> { return input.normal * sin(input.phase) * parameter(0u); }";

fn displaced() -> Effects {
    Effects {
        displacement_blocks: Some(0.5),
        scene: false,
        outline: None,
        bloom: None,
        particles: None,
        cells: None,
    }
}

#[test]
fn effects_stage_only_under_their_permission_and_with_their_functions() {
    let mut denied = granted(&[]);
    assert!(
        denied
            .set_material_with_effects(0, spec(WAVE), displaced())
            .unwrap()
            .is_err()
    );
    assert!(denied.commands.is_empty());
    let mut state = granted(&[Permission::RenderWorldMaterialVertex]);
    state
        .set_material_with_effects(0, spec(WAVE), displaced())
        .unwrap()
        .unwrap();
    let Some(Command::WorldMaterial {
        material: Some(staged),
        ..
    }) = state.commands.last()
    else {
        panic!("{:?}", state.commands);
    };
    assert_eq!(staged.effects.displacement_blocks, Some(0.5));
    // An outline without its function, or under the wrong permission, is refused.
    let outlined = Effects {
        outline: Some(Outline { width_pixels: 1.0 }),
        ..displaced()
    };
    assert!(
        state
            .set_material_with_effects(1, spec(WAVE), outlined)
            .unwrap()
            .is_err()
    );
    let mut passes = granted(&[
        Permission::RenderWorldMaterialVertex,
        Permission::RenderWorldMaterialPasses,
    ]);
    assert!(
        passes
            .set_material_with_effects(1, spec(WAVE), outlined)
            .unwrap()
            .unwrap_err()
            .contains("WGSL"),
        "the outline function is missing"
    );
    let with_outline = format!(
        "{WAVE}\nfn outline(input: WorldMaterialInput) -> vec3<f32> {{ return vec3<f32>(1.0); }}"
    );
    passes
        .set_material_with_effects(1, spec(&with_outline), outlined)
        .unwrap()
        .unwrap();
}

#[test]
fn computes_stage_under_passes_and_materials_read_only_held_cells() {
    const STEP: &str = "fn step(input: WorldCellInput) -> vec4<f32> { return cell(input.index) + vec4<f32>(compute_param(0u)); }";
    let compute = |cells| ComputeSpec {
        source: STEP.into(),
        cells,
        params: vec![1.0],
    };
    assert!(granted(&[]).set_compute(0, compute(64)).unwrap().is_err());
    let mut state = granted(&[Permission::RenderWorldMaterialPasses]);
    assert!(
        state
            .set_compute(
                0,
                compute(server_experience::world_material::MAX_COMPUTE_CELLS + 1)
            )
            .unwrap()
            .is_err()
    );
    let reader = "fn shade(input: WorldMaterialInput) -> Surface { return Surface(cell(3u).rgb, 1.0, vec3<f32>(0.0)); }";
    let reads = Effects {
        cells: Some(0),
        ..displaced()
    };
    let reads = Effects {
        displacement_blocks: None,
        ..reads
    };
    assert!(
        state
            .set_material_with_effects(1, spec(reader), reads)
            .unwrap()
            .is_err(),
        "no compute 0 yet"
    );
    state.set_compute(0, compute(64)).unwrap().unwrap();
    state
        .set_material_with_effects(1, spec(reader), reads)
        .unwrap()
        .unwrap();
    assert!(state.remove_compute(0).unwrap().is_err(), "cells in use");
    // Only new parameters spend no compile.
    let before = state.world_compiles;
    state
        .set_compute(
            0,
            ComputeSpec {
                params: vec![2.0],
                ..compute(64)
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(state.world_compiles, before);
    let hostile = ComputeSpec {
        source: "fn step(input: WorldCellInput) -> vec4<f32> { loop {} return vec4<f32>(0.0); }"
            .into(),
        ..compute(8)
    };
    assert!(state.set_compute(5, hostile).unwrap().is_err());
}
