//! Stateless blocks the pack defines literally: full cubes whose faces come from the block
//! texture map, and blocks vanilla draws nothing for.
//!
//! Anything that needs geometry the pack does not carry (campfire,
//! cauldron, ...) stays diagnostic until measured.

use assets::facts::{BlockRenderKind as R, block_render_fact};
use assets::{BlockFace, TerrainMarker, registry::BlockTraits};

use super::super::*;
use super::context::{RuleInputs, diagnostic_visual};
use super::dispatcher::CompileRuleResult;

/// Whether the state is a plain full cube whose six faces the terrain texture map names.
pub(in crate::compiler) fn is_literal_cube(record: &RegistryRecord) -> bool {
    record.canonical_state.as_ref() == "{}"
        && block_render_fact(&record.name).kind == R::LiteralCube
}

/// Whether vanilla terrain draws nothing for this block in normal play.
///
/// Invisible bedrock and moving blocks use the never-tessellated block shape; barriers, light
/// blocks and structure voids tessellate only into terrain layers drawn while a creative local
/// player holds that block. Neither kind is full, so neither culls a neighbour's face.
pub(in crate::compiler) fn is_default_invisible(name: &str) -> bool {
    matches!(
        block_render_fact(name).kind,
        R::Invisible | R::CameraSprite | R::StructureVoid
    )
}

/// The conditional terrain layer a barrier, light block or structure void draws into.
pub(in crate::compiler) fn terrain_marker(record: &RegistryRecord) -> Option<TerrainMarker> {
    match block_render_fact(&record.name).kind {
        R::CameraSprite if record.traits.contains(BlockTraits::LIGHT_BLOCK) => {
            Some(TerrainMarker::Light)
        }
        R::CameraSprite => Some(TerrainMarker::Barrier),
        R::StructureVoid => Some(TerrainMarker::StructureVoid),
        _ => None,
    }
}

/// The texture a marker draws on `face`: its world texture, or the carried art of a light
/// block, whose pack entry names no world texture.
pub(in crate::compiler) fn marker_descriptor(
    pack: &PackSources,
    record: &RegistryRecord,
    face: BlockFace,
) -> Option<(Descriptor, Box<str>)> {
    let key = match resolve_texture_key(&pack.blocks, record, face).key {
        Some(key) if key.as_ref() != pack_compile_input::pack::MISSING_TILE_TEXTURE_KEY => key,
        _ => pack_compile_input::pack::resolve_carried_face_key(&pack.blocks, record, face)?.into(),
    };
    let (path, state_variant) = pack.terrain.get_for_record_variant(&key, record)?;
    Some((
        Descriptor {
            state_variant,
            flipbook_selector: None,
            path: path.into(),
            texture_key: key.clone(),
            flags: MATERIAL_FLAG_ALPHA_CUTOUT,
        },
        key,
    ))
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
) -> CompileRuleResult {
    if is_default_invisible(&record.name) {
        let mut visual = diagnostic_visual(record);
        visual.flags.remove(
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE | BlockFlags::LEAF_MODEL,
        );
        visual.kind = VisualKind::Invisible;
        visual.support = VisualSupport::Exact;
        // The layer comes from the block alone, so a pack reload never moves a cell between
        // layers; a pack without the marker's art leaves it with nothing to draw.
        if let Some(marker) = terrain_marker(record) {
            visual.variant = marker.variant();
            for (slot, face) in visual.faces.iter_mut().zip(BlockFace::ALL) {
                if let Some(material) = inputs.material(record, face) {
                    *slot = material;
                }
            }
        }
        return CompileRuleResult::Compiled(visual);
    }
    if !is_literal_cube(record) {
        return CompileRuleResult::NoMatch;
    }
    let Some(materials) = inputs.materials(record) else {
        return CompileRuleResult::NoMatch;
    };
    let mut visual = diagnostic_visual(record);
    visual.flags |= BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
    visual.faces = materials;
    visual.kind = VisualKind::Cube;
    CompileRuleResult::Compiled(visual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_redstone_lamps_compile_as_exact_opaque_cubes() {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = assets::read_registry_for_protocol(
            &std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap()))
                .unwrap(),
            protocol,
        )
        .unwrap();
        let records = registry
            .into_iter()
            .filter(|record| {
                matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                    || is_literal_cube(record)
                        && matches!(
                            record.name.as_ref(),
                            "minecraft:redstone_lamp" | "minecraft:lit_redstone_lamp"
                        )
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
        std::fs::write(
            directory.path().join("blocks.json"),
            r#"{"stone":{"textures":"stone"},"redstone_lamp":{"textures":"lamp_off"},"lit_redstone_lamp":{"textures":"lamp_on"}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{"stone":{"textures":"textures/blocks/stone"},"lamp_off":{"textures":"textures/blocks/lamp_off"},"lamp_on":{"textures":"textures/blocks/lamp_on"}}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        for (name, color) in [
            ("stone", [100, 100, 100, 255]),
            ("lamp_off", [80, 40, 20, 255]),
            ("lamp_on", [200, 160, 80, 255]),
        ] {
            image::RgbaImage::from_pixel(assets::TILE_SIZE, assets::TILE_SIZE, image::Rgba(color))
                .save(directory.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let lights = vec![
            assets::LightProperties::default();
            records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        let (compiled, _) = compile_pack_inner(
            directory.path(),
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let lamps = records
            .iter()
            .filter(|record| record.name.ends_with("redstone_lamp"));
        assert_eq!(lamps.clone().count(), 2);
        for record in lamps {
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(visual.kind, VisualKind::Cube, "{}", record.name);
            assert_eq!(visual.support, VisualSupport::Exact, "{}", record.name);
            assert!(
                visual
                    .flags
                    .contains(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
            assert_eq!(visual.model_template, assets::NO_MODEL_TEMPLATE);
            for material in visual.faces {
                assert_ne!(material, DIAGNOSTIC_MATERIAL);
                assert_eq!(
                    compiled.materials[material as usize].flags, 0,
                    "{}",
                    record.name
                );
            }
        }
    }

    /// Light blocks name only carried art, which their in-world markers draw as vanilla's do.
    #[test]
    fn marker_blocks_compile_their_layer_and_art() {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
                .unwrap();
        let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let registry = assets::read_registry_for_protocol(
            &std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap()))
                .unwrap(),
            protocol,
        )
        .unwrap();
        let expected = [
            ("minecraft:barrier", TerrainMarker::Barrier),
            ("minecraft:structure_void", TerrainMarker::StructureVoid),
            ("minecraft:light_block_0", TerrainMarker::Light),
            ("minecraft:light_block_15", TerrainMarker::Light),
        ];
        let records = registry
            .into_iter()
            .filter(|record| {
                record.name.as_ref() == "minecraft:air"
                    || expected
                        .iter()
                        .any(|(name, _)| record.name.as_ref() == *name)
            })
            .collect::<Vec<_>>();
        let directory = tempfile::tempdir().unwrap();
        for folder in ["textures/blocks", "textures/items"] {
            std::fs::create_dir_all(directory.path().join(folder)).unwrap();
        }
        std::fs::write(
            directory.path().join("blocks.json"),
            r#"{"barrier":{"textures":"barrier"},"structure_void":{"textures":{"up":"structure_void_top","down":"flattened_structure_void","side":"flattened_structure_void"}},"light_block_0":{"carried_textures":"light_block_0"},"light_block_15":{"carried_textures":"light_block_15"}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{"barrier":{"textures":"textures/blocks/barrier"},"flattened_structure_void":{"textures":"textures/blocks/structure_void"},"structure_void_top":{"textures":"textures/blocks/structure_void_top"},"light_block_0":{"textures":"textures/items/light_block_0"},"light_block_15":{"textures":"textures/items/light_block_15"}}}"#,
        )
        .unwrap();
        std::fs::write(
            directory.path().join("textures/flipbook_textures.json"),
            "[]",
        )
        .unwrap();
        for (path, size, color) in [
            ("blocks/barrier", assets::TILE_SIZE, [200, 30, 30, 255]),
            (
                "blocks/structure_void",
                assets::TILE_SIZE,
                [220, 220, 220, 229],
            ),
            (
                "blocks/structure_void_top",
                assets::TILE_SIZE,
                [120, 220, 220, 229],
            ),
            ("items/light_block_0", 32, [20, 20, 20, 133]),
            ("items/light_block_15", 32, [250, 250, 120, 255]),
        ] {
            image::RgbaImage::from_pixel(size, size, image::Rgba(color))
                .save(directory.path().join(format!("textures/{path}.png")))
                .unwrap();
        }
        let lights = vec![
            assets::LightProperties::default();
            records
                .iter()
                .map(|record| record.sequential_id as usize + 1)
                .max()
                .unwrap()
        ];
        let (compiled, _) = compile_pack_inner(
            directory.path(),
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let mut light_materials = Vec::new();
        for (name, marker) in expected {
            let record = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap();
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(visual.kind, VisualKind::Invisible, "{name}");
            assert_eq!(
                TerrainMarker::from_variant(visual.kind, visual.variant),
                Some(marker),
                "{name}"
            );
            assert!(
                !visual
                    .flags
                    .intersects(BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
            );
            assert!(
                visual.faces.iter().all(|&face| face != DIAGNOSTIC_MATERIAL),
                "{name}"
            );
            let [west, east, down, up, north, south] = visual.faces;
            if marker == TerrainMarker::StructureVoid {
                // Boxes keep each face's own art.
                assert_ne!(up, down);
                assert!([west, east, north, south].iter().all(|&side| side == down));
            } else {
                // Sprites draw the up face's art.
                assert!(visual.faces.iter().all(|&face| face == up), "{name}");
            }
            if marker == TerrainMarker::Light {
                light_materials.push(visual.faces[0]);
            }
        }
        assert_ne!(light_materials[0], light_materials[1]);
    }

    #[test]
    fn invisible_names_cover_light_levels_zero_through_fifteen_only() {
        assert!(is_default_invisible("minecraft:barrier"));
        assert!(is_default_invisible("minecraft:light_block_0"));
        assert!(is_default_invisible("minecraft:light_block_15"));
        assert!(!is_default_invisible("minecraft:light_block_16"));
        assert!(!is_default_invisible("minecraft:light_block_x"));
        assert!(!is_default_invisible("minecraft:glass"));
    }
}
