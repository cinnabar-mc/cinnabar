use super::*;
use assets::{BlockFlags, BlockVisual, ModelQuad, ModelTemplate};

/// Builds original model fixtures with unchanged material ids and contiguous template parts;
/// both visuals draw the last part.
pub(super) fn model(kind: VisualKind, parts: &[(u32, u32)], quad_material: u32) -> RuntimeAssets {
    model_at(kind, parts, quad_material, parts.len() as u32 - 1)
}

/// [`model`] with both visuals drawing template `template`.
pub(super) fn model_at(
    kind: VisualKind,
    parts: &[(u32, u32)],
    quad_material: u32,
    template: u32,
) -> RuntimeAssets {
    model_with(kind, parts, &[quad_material], template, 0)
}

/// [`model_at`] whose quads cycle through `quad_materials` and whose non-diagnostic materials
/// carry `material_flags`.
pub(super) fn model_with(
    kind: VisualKind,
    parts: &[(u32, u32)],
    quad_materials: &[u32],
    template: u32,
    material_flags: u32,
) -> RuntimeAssets {
    let diagnostic = RuntimeAssets::diagnostic();
    let (corners, uvs) = assets::gui_item::cube_face(BlockFace::Up);
    let quad = ModelQuad {
        positions: corners.map(|point| point.map(|value| (value * 256.0) as i16)),
        uvs: uvs.map(|uv| uv.map(|value| (value * 4096.0) as u16)),
        material: quad_materials[0],
        flags: BlockFace::Up.model_quad_face_id(),
    };
    let mut visual = BlockVisual::diagnostic(BlockFlags::empty(), assets::ContributorRole::Primary);
    visual.kind = kind;
    visual.support = VisualSupport::Exact;
    visual.faces = [1, 1, 1, 2, 1, 1];
    visual.model_template = template;
    let mut quads = Vec::new();
    let templates = parts
        .iter()
        .map(|&(flags, count)| {
            let template = ModelTemplate {
                quad_start: quads.len() as u32,
                quad_count: count,
                flags,
            };
            let start = quads.len();
            quads.extend((start..start + count as usize).map(|index| ModelQuad {
                material: quad_materials[index % quad_materials.len()],
                ..quad
            }));
            template
        })
        .collect::<Vec<_>>();
    let compiled = assets::CompiledAssets {
        visuals: vec![visual; 2].into(),
        light_properties: vec![assets::LightProperties::OPAQUE_DARK; 2].into(),
        hashed: Box::new([]),
        materials: [0, material_flags, material_flags]
            .map(|flags| Material {
                flags,
                ..Material::unvaried()
            })
            .into(),
        model_templates: templates.into(),
        model_quads: quads.into(),
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

/// Supplies a complete binding whose new key never overrides a base terrain material.
fn binding_view() -> LayeredPackView {
    stack(&[(
        json!({"texture_data":{"replacement":{"textures":"textures/new"}}}),
        json!({"fixture":{"textures":{"west":"replacement","east":"replacement","down":"replacement","up":"replacement","north":"replacement","south":"replacement"},"carried_textures":"replacement"}}),
        vec![("textures/new", [10, 180, 30, 255])],
    )])
}

/// Excluded metadata must stop both world and carried preparation before publishing any output.
fn assert_binding_unchanged(base: &RuntimeAssets, mut keys: MaterialKeys) {
    let terrain = keys
        .terrain_entries()
        .map(|(key, value)| (key.into(), value.clone()))
        .collect::<Vec<_>>();
    let materials = keys
        .material_sources()
        .map(|(id, source)| (id, source.clone()))
        .collect::<Vec<_>>();
    let mut blocks = keys.block_sources().to_vec();
    blocks[0].cube_binding_eligible = false;
    keys = keys.with_sources(terrain, materials, blocks);
    let overlay = super::super::super::compile_block_overlay(
        &binding_view(),
        &CustomBlocks::default(),
        false,
        Some(&keys),
        assets::MAX_TEXTURE_LAYERS as u32,
        Some(base),
    );
    assert!(
        overlay.is_none(),
        "excluded binding must publish neither world faces nor carried art"
    );
}

#[test]
fn pottable_binding_preserves_the_pot_content_and_standalone_template_pair() {
    let base = model(
        VisualKind::Cross,
        &[(assets::MODEL_TEMPLATE_FLAG_POTTED_PLANT, 1), (0, 1)],
        1,
    );
    let before = base.potted_model_template(NetworkIdMode::Sequential, 0);
    assert_eq!(before, Some(0));
    assert_binding_unchanged(&base, keys());
    assert_eq!(
        base.potted_model_template(NetworkIdMode::Sequential, 0),
        before
    );
}

#[test]
fn directional_binding_keeps_the_existing_world_face_mapping() {
    assert_binding_unchanged(&base(), keys());
}

#[test]
fn container_binding_preserves_internal_material_ownership() {
    let base = model(VisualKind::Model, &[(0, 1)], 1);
    assert_ne!(
        base.model_quads()[0].material,
        base.resolve(NetworkIdMode::Sequential, 0)
            .face(BlockFace::Up)
            .material_id()
    );
    assert_binding_unchanged(&base, keys());
}

#[test]
fn tinted_carried_binding_leaves_both_routes_unchanged() {
    let view = stack(&[(
        json!({"texture_data":{"plain":{"textures":"textures/plain"},"tinted":{"textures":{"path":"textures/tinted","tint_color":"#ff0000"}}}}),
        json!({"fixture":{"textures":"plain","carried_textures":"tinted"}}),
        vec![
            ("textures/plain", [80, 100, 120, 255]),
            ("textures/tinted", [80, 100, 120, 255]),
        ],
    )]);
    assert!(
        super::super::super::compile_block_overlay(
            &view,
            &CustomBlocks::default(),
            false,
            Some(&keys()),
            assets::MAX_TEXTURE_LAYERS as u32,
            Some(&base())
        )
        .is_none()
    );
    assert_binding_unchanged(&base(), keys());
}

#[test]
fn wall_binding_keeps_the_empty_placement_model_and_existing_inventory_route() {
    let base = model(VisualKind::Model, &[(0, 0)], 1);
    assert!(base.model_quads().is_empty());
    assert_binding_unchanged(&base, keys());
}

#[test]
fn terrain_replacement_still_applies_when_block_binding_is_excluded() {
    let base = model(VisualKind::Model, &[(0, 1)], 1);
    let keys = keys();
    let mut blocks = keys.block_sources().to_vec();
    blocks
        .iter_mut()
        .for_each(|source| source.cube_binding_eligible = false);
    let terrain = keys
        .terrain_entries()
        .map(|(key, value)| (key.into(), value.clone()))
        .collect::<Vec<_>>();
    let materials = keys
        .material_sources()
        .map(|(id, source)| (id, source.clone()))
        .collect::<Vec<_>>();
    let keys = keys.with_sources(terrain, materials, blocks);
    let pixel = [140, 20, 90, 255];
    let view = stack(&[(
        json!({"texture_data":{"state":{"textures":["textures/new","textures/new"]},"replacement":{"textures":"textures/other"}}}),
        json!({"fixture":{"textures":"replacement","carried_textures":"replacement"}}),
        vec![
            ("textures/new", pixel),
            ("textures/other", [0, 255, 0, 255]),
        ],
    )]);
    let overlay = compile(&view, &base, &keys);
    assert!(overlay.overlay.face_overrides.is_empty());
    assert!(overlay.vanilla_carried.is_empty());
    let runtime = base.with_block_overlay(2, &overlay.overlay).unwrap();
    for id in 0..base.visual_count() as u32 {
        assert_eq!(
            runtime.resolve(NetworkIdMode::Sequential, id),
            base.resolve(NetworkIdMode::Sequential, id),
        );
    }
    assert_eq!(runtime.model_templates(), base.model_templates());
    assert_eq!(runtime.model_quads(), base.model_quads());
    assert_eq!(material_color(&runtime, 1), pixel);
}
