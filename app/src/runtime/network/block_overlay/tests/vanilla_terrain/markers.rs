//! Session packs rebind the art of invisible terrain markers as the offline compiler does.

use super::*;
use assets::TerrainMarker;

/// A barrier, a light block and a structure void, each drawn with its own base material.
fn marker_base() -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let marker = |class: TerrainMarker, material: u32| {
        let mut visual = super::super::super::diagnostic_visual();
        visual.kind = VisualKind::Invisible;
        visual.support = VisualSupport::Exact;
        visual.variant = class.variant();
        visual.faces = [material; 6];
        visual
    };
    let cutout = Material {
        flags: assets::MATERIAL_FLAG_ALPHA_CUTOUT,
        ..Material::unvaried()
    };
    let compiled = assets::CompiledAssets {
        visuals: vec![
            marker(TerrainMarker::Barrier, 1),
            marker(TerrainMarker::Light, 2),
            marker(TerrainMarker::StructureVoid, 3),
        ]
        .into(),
        light_properties: vec![assets::LightProperties::default(); 3].into(),
        hashed: Box::new([]),
        materials: vec![Material::unvaried(), cutout, cutout, cutout].into(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: diagnostic.texture_pages().to_vec().into(),
        biomes: assets::CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [1; 32],
            block_registry_sha256: [2; 32],
            light_registry_sha256: [3; 32],
            biome_registry_sha256: [4; 32],
        },
    };
    RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap()
}

/// Carrier sources for the three markers, named as the vanilla pack names their art. The
/// base pack gives barriers and structure voids world art and light blocks only carried art.
fn marker_keys() -> MaterialKeys {
    let entries = [
        (1, "barrier", "textures/blocks/barrier"),
        (2, "light_block_15", "textures/items/light_block_15"),
        (3, "structure_void", "textures/blocks/structure_void"),
    ];
    let block = |visual: u32, name: &str, world: bool| BlockTextureSource {
        visual,
        name: format!("minecraft:{name}").into(),
        binding_name: name.into(),
        source_faces: [0, 1, 2, 3, 4, 5],
        source_rotations: [0; 6],
        cube_binding_eligible: false,
        carried_keys: std::array::from_fn(|_| (!world).then(|| name.into())),
        carried_fallback_keys: std::array::from_fn(|_| world.then(|| name.into())),
    };
    MaterialKeys::from_entries(entries.map(|(id, key, _)| (id, key)))
        .with_aliases(entries.map(|(_, key, path)| (key, path)))
        .with_sources(
            entries.map(|(_, key, path)| (key.into(), json!({ "textures": path }))),
            entries.map(|(id, key, path)| {
                (
                    id,
                    MaterialTextureSource {
                        key: key.into(),
                        variant: 0,
                        path: path.into(),
                        raster: format!("{path}.png").into(),
                        baked_tint: None,
                    },
                )
            }),
            [
                block(0, "barrier", true),
                block(1, "light_block_15", false),
                block(2, "structure_void", true),
            ],
        )
}

/// The material a marker's face draws with after the overlay.
fn face_material(runtime: &RuntimeAssets, visual: u32, face: BlockFace) -> u32 {
    runtime
        .resolve(NetworkIdMode::Sequential, visual)
        .face(face)
        .material_id()
}

/// A server pack that points a marker at a new texture key changes the art the marker layer
/// draws, without touching its class or the base carrier.
#[test]
fn session_packs_rebind_marker_art_to_new_texture_keys() {
    let base = marker_base();
    let keys = marker_keys();
    let barrier = [200, 20, 20, 255];
    let light = [240, 230, 60, 255];
    let void_top = [20, 200, 220, 255];
    let void_side = [120, 120, 220, 255];
    let view = stack(&[(
        json!({"texture_data":{
            "red_sign":{"textures":"textures/red_sign"},
            "bright_bulb":{"textures":"textures/bright_bulb"},
            "void_top":{"textures":"textures/void_top"},
            "void_side":{"textures":"textures/void_side"},
        }}),
        json!({
            "barrier":{"textures":"red_sign"},
            "light_block_15":{"carried_textures":"bright_bulb"},
            "structure_void":{"textures":{"up":"void_top","down":"void_side","side":"void_side"}},
        }),
        vec![
            ("textures/red_sign", barrier),
            ("textures/bright_bulb", light),
            ("textures/void_top", void_top),
            ("textures/void_side", void_side),
        ],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(3, &overlay.overlay).unwrap();

    for (visual, class) in [
        (0, TerrainMarker::Barrier),
        (1, TerrainMarker::Light),
        (2, TerrainMarker::StructureVoid),
    ] {
        let block = runtime.resolve(NetworkIdMode::Sequential, visual);
        assert_eq!(
            TerrainMarker::from_variant(block.kind(), block.variant()),
            Some(class)
        );
    }
    for face in BlockFace::ALL {
        assert_eq!(
            material_color(&runtime, face_material(&runtime, 0, face)),
            barrier
        );
        assert_eq!(
            material_color(&runtime, face_material(&runtime, 1, face)),
            light
        );
        let expected = if face == BlockFace::Up {
            void_top
        } else {
            void_side
        };
        assert_eq!(
            material_color(&runtime, face_material(&runtime, 2, face)),
            expected,
            "{face:?}"
        );
    }
    assert_eq!(face_material(&base, 0, BlockFace::Up), 1);
}

/// Carried-only overrides change a marker's art only when no pack gives the block world art,
/// so the world art a session inherits from the base pack stays in place.
#[test]
fn carried_only_overrides_keep_inherited_world_marker_art() {
    let base = marker_base();
    let keys = marker_keys();
    let light = [240, 230, 60, 255];
    let view = stack(&[(
        json!({"texture_data":{
            "red_sign":{"textures":"textures/red_sign"},
            "bright_bulb":{"textures":"textures/bright_bulb"},
        }}),
        json!({
            "barrier":{"carried_textures":"red_sign"},
            "light_block_15":{"carried_textures":"bright_bulb"},
            "structure_void":{"carried_textures":"red_sign"},
        }),
        vec![
            ("textures/red_sign", [200, 20, 20, 255]),
            ("textures/bright_bulb", light),
        ],
    )]);
    let overlay = compile(&view, &base, &keys);
    let runtime = base.with_block_overlay(3, &overlay.overlay).unwrap();

    for face in BlockFace::ALL {
        assert_eq!(face_material(&runtime, 0, face), 1, "{face:?}");
        assert_eq!(face_material(&runtime, 2, face), 3, "{face:?}");
        assert_eq!(
            material_color(&runtime, face_material(&runtime, 1, face)),
            light
        );
    }
}
