use super::*;
use crate::block_entity::{
    BlockEntityKind, BlockEntityScene, BlockEntitySubmission, MIN_DYNAMIC_ATLAS_WIDTH, SceneClock,
};

/// Installs a synthetic 7x1 rope texture without relying on local Mojang assets.
fn scene(with_rope: bool) -> BlockEntityScene {
    let placements = [assets::BlockEntityPlacement {
        name: assets::LOYALTY_ROPE_TEXTURE.into(),
        x: 0,
        y: 0,
        width: 7,
        height: 1,
    }];
    let encoded = assets::encode_block_entity_catalog(
        b"{}",
        MIN_DYNAMIC_ATLAS_WIDTH,
        16,
        &vec![255; MIN_DYNAMIC_ATLAS_WIDTH as usize * 16 * 4],
        if with_rope { &placements[..] } else { &[] },
    )
    .unwrap();
    let mut scene = BlockEntityScene::default();
    scene.install_assets(&assets::RuntimeBlockEntityAssets::decode(&encoded).unwrap());
    scene
}

fn rope(return_ticks: u32) -> LoyaltyRopeModel {
    LoyaltyRopeModel {
        trident: [0.0; 3],
        span: [0.0, 0.0, 10.0],
        owner_yaw_degrees: 0.0,
        phase: 0.0,
        return_ticks,
    }
}

fn draw(scene: &mut BlockEntityScene, model: LoyaltyRopeModel) -> Vec<BlockEntityVertex> {
    scene
        .update(
            SceneClock::default(),
            &[],
            &[BlockEntitySubmission {
                block: [0; 3],
                light: 1.0.into(),
                kind: BlockEntityKind::LoyaltyRope(model),
            }],
        )
        .solid
        .to_vec()
}

/// Strip vertex `side` (0 or 1) of segment boundary `segment`, by vanilla's formula with
/// exact trigonometry, for a straight span along +Z with zero yaw and phase.
fn expected(strip: usize, segment: u32, side: usize, length: f32) -> Vec3 {
    let t = segment as f32 / 37.0;
    let sag = (1.0 - (2.0 * t - 1.0).powi(2)) * (length / 30.0).min(0.5);
    let coil = t * std::f32::consts::PI * 8.0;
    let ripple = [0.02, 0.01][strip];
    let centre = Vec3::new(
        coil.sin() * sag,
        sag + 0.1 + coil.cos() * ripple,
        coil.sin() * sag + length * t,
    );
    let width = sag * 0.1 * side as f32;
    centre
        + if strip == 0 {
            Vec3::new(width, width, 0.0)
        } else {
            Vec3::new(width, 0.0, width)
        }
}

#[test]
fn the_rope_grows_two_segments_a_returning_tick_up_to_the_full_span() {
    let mut scene = scene(true);
    for (ticks, segments) in [(0, 1), (1, 3), (5, 11), (17, 35), (18, 37), (400, 37)] {
        assert_eq!(
            draw(&mut scene, rope(ticks)).len(),
            2 * segments * 6,
            "{ticks} returning ticks"
        );
    }
}

#[test]
fn both_strips_sag_and_coil_along_the_span_as_vanilla_draws_them() {
    let mut scene = scene(true);
    let vertices = draw(&mut scene, rope(18));
    let per_strip = 37 * 6;
    for strip in 0..2 {
        for segment in 0..37 {
            let quad = &vertices[(strip * per_strip + segment * 6)..][..6];
            // Each segment is the strip triangles (a, b, c) and (c, b, d).
            let corners = [
                (segment, 0),
                (segment, 1),
                (segment + 1, 0),
                (segment + 1, 1),
            ];
            for (vertex, corner) in quad.iter().zip([0, 1, 2, 2, 1, 3]) {
                let (boundary, side) = corners[corner];
                let want = expected(strip, boundary as u32, side, 10.0);
                let got = Vec3::from_array(vertex.position);
                assert!(
                    got.distance(want) < 2e-3,
                    "strip {strip} segment {segment}: {got} vs {want}"
                );
            }
        }
    }
    // The owner end sits 0.1 above the span's end, where the sag returns to zero.
    let last = Vec3::from_array(vertices[per_strip - 1].position);
    assert!(last.distance(Vec3::new(0.0, 0.12, 10.0)) < 1e-3, "{last}");
}

#[test]
fn the_texture_repeats_every_seven_segments_along_its_last_row() {
    let mut scene = scene(true);
    let vertices = draw(&mut scene, rope(18));
    let atlas = scene.atlas().unwrap();
    let size = atlas.size().map(|side| side as f32);
    let rect = atlas
        .texture(assets::LOYALTY_ROPE_TEXTURE, [1.0; 2])
        .unwrap()
        .rect;
    for segment in 0..37_usize {
        let first = vertices[segment * 6];
        let u = (segment % 7) as f32 / 7.0;
        assert!((first.uv[0] - (rect.x + rect.width * u) / size[0]).abs() < 1e-6);
        let row = first.uv[1] * size[1];
        assert!(row > rect.y && row < rect.y + rect.height, "{row}");
    }
}

#[test]
fn a_rope_without_its_texture_or_with_non_finite_input_draws_nothing() {
    assert!(draw(&mut scene(false), rope(18)).is_empty());
    let mut scene = scene(true);
    for broken in [
        LoyaltyRopeModel {
            phase: f32::NAN,
            ..rope(18)
        },
        LoyaltyRopeModel {
            span: [0.0, f32::INFINITY, 0.0],
            ..rope(18)
        },
    ] {
        assert!(draw(&mut scene, broken).is_empty());
    }
}
