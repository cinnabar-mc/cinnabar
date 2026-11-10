use super::*;
use crate::{system::ParticleSystem, world::EmptyWorld};

fn tile() -> TileRequest {
    TileRequest {
        key: 1,
        size: 16,
        pixels: vec![255; 16 * 16 * 4].into(),
    }
}

fn variable(request: &SpawnRequest, name: &str) -> f32 {
    request
        .variables
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| panic!("{name} is bound"))
}

#[test]
fn terrain_effect_counts_cube_low_intensities_and_cap_high_ones() {
    assert_eq!(terrain_effect_particles(1.0), 1.0);
    assert_eq!(terrain_effect_particles(1.5), 3.375);
    assert_eq!(terrain_effect_particles(2.0), 4.0);
    assert_eq!(terrain_effect_particles(3.0), 18.0);
    assert_eq!(terrain_effect_particles(100.0), 255.0);
}

#[test]
fn top_face_dust_spreads_wider_and_faster_than_side_dust() {
    let hit = [3.25, 65.0, -7.5];
    let top = brush_dust_request(
        "minecraft:test_terrain",
        [3, 64, -8],
        1,
        hit,
        tile(),
        [1.0; 4],
    );
    let side = brush_dust_request(
        "minecraft:test_terrain",
        [3, 64, -8],
        2,
        hit,
        tile(),
        [1.0; 4],
    );
    assert_eq!(top.position, hit);
    assert_eq!(side.position, hit);
    assert!(variable(&top, "emitter_radius") > variable(&side, "emitter_radius"));
    assert!(variable(&top, "velocity_scalar") > variable(&side, "velocity_scalar"));
    for request in [&top, &side] {
        let count = variable(request, "emitter_particles_count");
        assert!((7.0..8.0).contains(&count), "{count}");
        assert!((variable(request, "emitter_intensity").powi(3) - count).abs() < 1e-3);
    }
}

#[test]
fn one_stroke_draws_seven_block_textured_pieces() {
    let mut system = ParticleSystem::default();
    // Synthetic emitter checks the real request through the live engine.
    assert!(system.register_effect(br#"{"particle_effect":{"description":{
      "identifier":"minecraft:test_terrain","basic_render_parameters":{"material":"particles_alpha","texture":"atlas.terrain"}},
      "components":{"minecraft:emitter_lifetime_once":{"active_time":1},
      "minecraft:emitter_rate_instant":{"num_particles":"variable.emitter_particles_count"},
      "minecraft:particle_appearance_billboard":{"size":[0.1,0.1]},
      "minecraft:particle_lifetime_expression":{"max_lifetime":1}}}}"#));
    let request = brush_dust_request(
        "minecraft:test_terrain",
        [0, 0, 0],
        4,
        [0.0, 0.5, 0.5],
        tile(),
        [1.0; 4],
    );
    assert!(system.spawn_terrain(&request).is_some());
    system.tick(0.01, &EmptyWorld);
    assert_eq!(system.live_particles(), 7);
}
