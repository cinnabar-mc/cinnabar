//! Synthetic liquid scenes shared by sampler checks and throughput benchmarks.

use assets::{
    BlockFlags, BlockOverlay, BlockVisual, ContributorRole, LightProperties,
    MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_LIQUID_DEPTH_WRITE,
    MATERIAL_FLAG_WATER_TINT, Material, NO_ANIMATION, NO_MODEL_TEMPLATE, RuntimeAssets, TextureRef,
    VisualKind, VisualSupport,
};
use world::{RawBlockIds, SubChunk};

pub const AIR: u32 = 0;
pub const LIQUID_STATES: u32 = 320;
pub const SOLID: u32 = LIQUID_STATES + 1;
pub const GLASS: u32 = SOLID + 1;
pub const INVALID_LEVEL: u32 = GLASS + 1;
pub const UNSUPPORTED: u32 = INVALID_LEVEL + 1;
pub const SCENES: [&str; 6] = [
    "ocean",
    "surface",
    "flowing_mixed",
    "sparse",
    "dry",
    "diverse",
];

/// Builds distinct liquid families, all levels, opaque contacts and unsupported liquid states.
pub fn runtime_assets() -> RuntimeAssets {
    let base = RuntimeAssets::diagnostic();
    let mut texture = base.texture_array().clone();
    texture.layers = 1;
    let liquid_materials = LIQUID_STATES as usize / 16 * 6;
    let mut materials: Vec<_> = (0..liquid_materials)
        .map(|index| Material {
            texture: TextureRef::new(1, 0).unwrap(),
            flags: if index / 6 == 1 {
                MATERIAL_FLAG_LIQUID_DEPTH_WRITE
            } else {
                MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT
            },
            ..Material::unvaried()
        })
        .collect();
    let opaque = materials.len() as u32;
    materials.push(Material {
        texture: TextureRef::new(1, 0).unwrap(),
        ..Material::unvaried()
    });
    let cutout = materials.len() as u32;
    materials.push(Material {
        texture: TextureRef::new(1, 0).unwrap(),
        flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        ..Material::unvaried()
    });
    let visual = BlockVisual {
        native_material: assets::native_material::NativeMaterialFacts::Unresolved,
        support: VisualSupport::Exact,
        faces: [opaque; 6],
        flags: BlockFlags::empty(),
        kind: VisualKind::Liquid,
        contributor_role: ContributorRole::LiquidAdditional,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
        destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
    };
    let mut visuals: Vec<_> = (0..LIQUID_STATES)
        .map(|index| BlockVisual {
            faces: std::array::from_fn(|face| index / 16 * 6 + face as u32),
            variant: index % 16,
            ..visual
        })
        .collect();
    for faces in [[opaque; 6], [cutout; 6]] {
        visuals.push(BlockVisual {
            faces,
            flags: BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            kind: VisualKind::Cube,
            contributor_role: ContributorRole::Primary,
            ..visual
        });
    }
    visuals.push(BlockVisual {
        variant: 16,
        faces: [0; 6],
        ..visual
    });
    visuals.push(visual);
    assert_eq!(visuals.len(), UNSUPPORTED as usize);
    base.with_block_overlay(
        1,
        &BlockOverlay {
            light_properties: vec![LightProperties::default(); visuals.len()],
            visuals,
            materials,
            textures: vec![texture],
            ..Default::default()
        },
    )
    .unwrap()
}

/// Returns one section in storage order, with network values chosen by local coordinate.
pub fn values(id_at: impl Fn([u8; 3]) -> u32) -> Vec<u32> {
    (0..4096)
        .map(|linear| {
            id_at([
                (linear >> 8) as u8,
                (linear & 15) as u8,
                ((linear >> 4) & 15) as u8,
            ])
        })
        .collect()
}

/// Encodes each layer as a palette-packed network storage without external game fixtures.
pub fn chunk(layers: &[Vec<u32>]) -> SubChunk {
    let mut payload = vec![9, layers.len() as u8, 0];
    for layer in layers {
        assert_eq!(layer.len(), 4096);
        let mut palette = layer.clone();
        palette.sort_unstable();
        palette.dedup();
        payload.push(33); // Network storage with 16 bits per palette index.
        for pair in layer.as_chunks::<2>().0 {
            let low = palette.binary_search(&pair[0]).unwrap() as u32;
            let high = palette.binary_search(&pair[1]).unwrap() as u32;
            payload.extend((low | high << 16).to_le_bytes());
        }
        varint(palette.len() as u32 * 2, &mut payload);
        for id in palette {
            varint((id << 1) ^ ((id as i32 >> 31) as u32), &mut payload);
        }
    }
    SubChunk::decode(&payload, &RawBlockIds { air: AIR })
}

/// Creates dense, surface, mixed, sparse, dry or cache-overflow benchmark input.
pub fn scene(name: &str) -> SubChunk {
    chunk(&[values(|[x, y, z]| match name {
        "ocean" => 1,
        "surface" if y < 8 => 1,
        "flowing_mixed" if y < 4 + (x + z) % 6 => {
            1 + u32::from((x + y + z) % 16) + if x < 8 { 0 } else { 16 }
        }
        "flowing_mixed" if y == 4 + (x + z) % 6 => {
            if x % 3 == 0 {
                SOLID
            } else {
                GLASS
            }
        }
        "sparse" if [x, y, z] == [8, 8, 8] => 1,
        "dry" if y < 8 => SOLID,
        "diverse" => 1 + ((u32::from(x) << 8) | (u32::from(z) << 4) | u32::from(y)) % LIQUID_STATES,
        _ => AIR,
    })])
}

/// Appends an unsigned base-128 integer to a synthetic chunk payload.
fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 127) as u8;
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return;
        }
    }
}
