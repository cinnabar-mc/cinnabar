use super::*;

/// A selector with a quarter-weight and a three-quarter-weight alternative, as the terrain
/// shader's GPU fixture lays it out.
fn weighted() -> [Material; 3] {
    let leaf = |layer, weight: f32| Material {
        texture: TextureRef::new(0, layer).unwrap(),
        variation_weight: weight.to_bits(),
        ..Material::unvaried()
    };
    [
        Material {
            variation_start: 1,
            variation_count: 2,
            ..Material::unvaried()
        },
        leaf(1, 0.25),
        leaf(2, 0.75),
    ]
}

/// The CPU selection picks the same alternatives the GPU fixture
/// `positional_material_gpu_matches_signed_coordinate_vectors` reads back.
#[test]
fn positional_materials_pick_the_shaders_weighted_alternative() {
    let materials = weighted();
    let picks = [
        [0, 0, 0],
        [-1, 0, -1],
        [16, 64, -17],
        [123_456, -64, 98_765],
    ]
    .map(|position| positional_material_id(&materials, 0, position));
    assert_eq!(picks, [1, 2, 2, 2]);

    let mut single = weighted();
    single[0].variation_count = 1;
    assert_eq!(positional_material_id(&single, 0, [5, 6, 7]), 1);
    assert_eq!(positional_material_id(&materials, 1, [5, 6, 7]), 1);
    assert_eq!(positional_material_id(&materials, 9, [5, 6, 7]), 9);
}
