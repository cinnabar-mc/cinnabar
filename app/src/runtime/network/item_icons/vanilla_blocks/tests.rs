use super::*;
use crate::runtime::network::block_overlay::{CarriedBlock, OverlayGaps};
use assets::{
    BlockOverlay, BlockVisual, Material, ModelQuad, ModelTemplate, NO_ANIMATION, TextureRef,
};

/// Original cube and half-height model fixtures share the same texture contract.
fn block(model: bool) -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let quads = assets::BlockFace::ALL.map(|face| {
        let (corners, uvs) = assets::gui_item::cube_face(face);
        ModelQuad {
            positions: corners.map(|point| {
                [
                    (point[0] * 256.0) as i16,
                    (point[1] * 128.0) as i16,
                    (point[2] * 256.0) as i16,
                ]
            }),
            uvs: uvs.map(|uv| uv.map(|value| (value * 4096.0) as u16)),
            material: 0,
            flags: face.model_quad_face_id(),
        }
    });
    let overlay = BlockOverlay {
        visuals: vec![BlockVisual {
            faces: [0; 6],
            flags: if model {
                assets::BlockFlags::empty()
            } else {
                assets::BlockFlags::CUBE_GEOMETRY
            },
            kind: if model {
                VisualKind::Model
            } else {
                VisualKind::Cube
            },
            support: assets::VisualSupport::Exact,
            contributor_role: assets::ContributorRole::Primary,
            model_template: if model { 0 } else { assets::NO_MODEL_TEMPLATE },
            animation: NO_ANIMATION,
            variant: 0,
            native_material: assets::native_material::NativeMaterialFacts::Unresolved,
            destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
        }],
        light_properties: vec![assets::LightProperties::OPAQUE_DARK],
        materials: vec![Material {
            texture: TextureRef::new(1, 0).unwrap(),
            ..Material::unvaried()
        }],
        model_templates: if model {
            vec![ModelTemplate {
                quad_start: 0,
                quad_count: quads.len() as u32,
                flags: 0,
            }]
        } else {
            vec![]
        },
        model_quads: if model { quads.into() } else { vec![] },
        textures: vec![diagnostic.texture_array().clone()],
        ..Default::default()
    };
    diagnostic.with_block_overlay(1, &overlay).unwrap()
}

/// Only the optional carried art changes; the slab's geometry comes from the base.
fn textures() -> CompiledBlockOverlay {
    let mut texture = RuntimeAssets::diagnostic().texture_array().clone();
    for mip in &mut texture.mips {
        mip.rgba8 = [80, 150, 220, 255]
            .repeat((mip.size * mip.size) as usize)
            .into();
    }
    CompiledBlockOverlay {
        overlay: BlockOverlay {
            materials: vec![
                Material::unvaried(),
                Material {
                    texture: TextureRef::new(1, 0).unwrap(),
                    ..Material::unvaried()
                },
            ],
            textures: vec![texture],
            ..Default::default()
        },
        gaps: OverlayGaps::default(),
        vanilla_carried: vec![CarriedBlock {
            visual: 1,
            faces: [1; 6],
        }],
    }
}

#[test]
fn model_routes_never_publish_rebound_inventory_or_held_art() {
    let base = block(true);
    let mut output = BlockIcons::default();
    let route = Route {
        identifier: "minecraft:fixture_slab".into(),
        metadata: 7,
        visual: 1,
    };
    append_routes(&base, &textures(), &mut output, &[route]);
    assert!(output.icons.is_empty());
    assert!(output.block_sheets.is_empty());
}

#[test]
fn plain_cube_routes_publish_both_thumbnail_and_held_sheet() {
    let mut output = BlockIcons::default();
    let route = Route {
        identifier: "minecraft:fixture_cube".into(),
        metadata: 7,
        visual: 1,
    };
    append_routes(&block(false), &textures(), &mut output, &[route]);
    assert_eq!(output.icons.len(), 1);
    assert_eq!(output.block_sheets.len(), 1);
    assert_eq!(output.icons[0].metadata, 7);
    assert_eq!(output.block_sheets[0].metadata, 7);
}

#[test]
fn hd_carried_sheets_survive_session_publication_without_thumbnails() {
    let mut compiled = textures();
    let side = 256;
    let color = [80, 150, 220, 255];
    compiled.overlay.textures.first_mut().unwrap().mips =
        assets::build_legacy_terrain_mip_chain(&color.repeat((side * side) as usize), side)
            .unwrap()
            .into();
    let mut output = BlockIcons::default();
    let route = Route {
        identifier: "minecraft:fixture_cube".into(),
        metadata: 7,
        visual: 1,
    };
    append_routes(&block(false), &compiled, &mut output, &[route]);
    assert!(output.icons.is_empty());
    assert!(output.misses.is_empty());
    assert_eq!(output.block_sheets.len(), 1);
    let expected = output.block_sheets[0].rgba8.clone();
    let icons = super::super::compile_session_icons(
        &super::super::tests::view(),
        &[("minecraft:fixture_cube".into(), "test:gem".into())],
        output,
    )
    .unwrap();
    assert!(icons.icons.is_empty());
    assert!(icons.misses.is_empty());
    assert_eq!(icons.block_sheets.len(), 1);
    assert_eq!(icons.block_sheets[0].metadata, 7);
    assert_eq!(icons.block_sheets[0].rgba8, expected);
    assert_eq!(icons.block_material_flags["minecraft:fixture_cube"], 0);
}

#[test]
fn full_thumbnail_capacity_does_not_discard_available_carried_sheets() {
    let mut output = BlockIcons {
        icons: (0..MAX_SESSION_ICONS)
            .map(|_| SessionIcon {
                identifier: "test:other".into(),
                metadata: 0,
                width: 1,
                height: 1,
                rgba8: vec![255; 4].into(),
            })
            .collect(),
        ..Default::default()
    };
    let route = Route {
        identifier: "minecraft:fixture_cube".into(),
        metadata: 7,
        visual: 1,
    };
    append_routes(&block(false), &textures(), &mut output, &[route]);
    assert_eq!(output.icons.len(), MAX_SESSION_ICONS);
    assert_eq!(output.block_sheets.len(), 1);
    assert_eq!(output.block_sheets[0].metadata, 7);
}
