//! Bounded material inspection: why each published entity and block material draws as it does.
//! Lists names, pack-relative sources and states only, never pixels, host paths or handles.

use bevy::prelude::World;
use serde_json::{Value, json};

use crate::runtime::world::ClientWorld;

/// Most entity or block materials one inspection lists.
const MAX_LISTED_MATERIALS: usize = 256;

/// Each material the session's server pack actors draw: its source, inheritance chain, selected
/// variant, applied and carried defines, bound texture slots, render state and why it is not
/// drawn exactly.
pub(super) fn entity_materials(world: &World) -> Value {
    let Some(pack) = world
        .get_resource::<ClientWorld>()
        .and_then(|client| client.pack_entities.as_ref())
    else {
        return json!({ "server_pack": false });
    };
    let Some(report) = &pack.material_report else {
        return json!({ "server_pack": true, "report": false });
    };
    let render = pack.assets.render_data();
    let listed = report
        .materials
        .iter()
        .take(MAX_LISTED_MATERIALS)
        .map(|material| {
            let definition = render.materials.get(material.definition as usize);
            let state = render
                .layers
                .iter()
                .find(|layer| layer.material_definition == Some(material.definition))
                .and_then(|layer| layer.material_state);
            let slots = std::iter::once(0)
                .chain(
                    state
                        .map_or(&[] as &[usize], assets::EntityRenderMaterialState::extra_texture_slots)
                        .iter()
                        .copied(),
                )
                .collect::<Vec<_>>();
            json!({
                "name": material.name,
                "definition": material.definition,
                "source": material.source,
                "chain": material.chain,
                "variant": material.variant,
                "applied_defines": material.applied_defines,
                "carried_defines": material.carried_defines,
                "texture_slots": slots,
                "shading": state,
                "render_state": definition.map(|definition| format!("{:?}", definition.render_state())),
                "reason": material.reason,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "server_pack": true,
        "report": true,
        "total": report.materials.len(),
        "omitted": report.materials.len().saturating_sub(listed.len()),
        "materials": listed,
    })
}

/// The session's server block materials: those drawing overlay textures, with their draw flags
/// and masks.
pub(super) fn block_materials(world: &World) -> Value {
    let Some(client) = world.get_resource::<ClientWorld>() else {
        return json!({ "available": false });
    };
    let materials = client.runtime_assets.materials();
    let overlay = materials
        .iter()
        .enumerate()
        .filter(|(_, material)| material.texture.page() > 0);
    let total = overlay.clone().count();
    let listed = overlay
        .take(MAX_LISTED_MATERIALS)
        .map(|(id, material)| {
            let flag = |flag: u32| material.flags & flag != 0;
            json!({
                "id": id,
                "texture": { "page": material.texture.page(), "layer": material.texture.layer() },
                "blend": flag(assets::MATERIAL_FLAG_ALPHA_BLEND),
                "cutout": flag(assets::MATERIAL_FLAG_ALPHA_CUTOUT),
                "variations": material.variation_count,
                "glow_mask": material.glow_mask != assets::NO_MASK,
                "emissive_mask": material.emissive_mask != assets::NO_MASK,
                "ao_exponent": f32::from_bits(material.ao_exponent),
                "enhanced_surfaces": client.runtime_assets.server_enhanced().is_some_and(|maps| {
                    maps.material_reference(id as u32, material.texture) != u32::MAX
                }),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "available": true,
        "total": total,
        "omitted": total.saturating_sub(listed.len()),
        "materials": listed,
    })
}

#[cfg(test)]
mod tests {
    use std::{io::Write, sync::Arc};

    use super::*;

    /// A one-entity server pack whose controller draws `example:glow` deriving a client base.
    fn pack() -> Arc<assets::SessionEntityPack> {
        let png = {
            let image = image::RgbaImage::from_pixel(16, 16, image::Rgba([90, 90, 90, 255]));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            bytes.into_inner()
        };
        let id = "00000000-0000-0000-0000-000000000061";
        let files: Vec<(&str, Vec<u8>)> = vec![
            (
                "manifest.json",
                json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
                "modules":[{"type":"resources"}]})
                .to_string()
                .into_bytes(),
            ),
            (
                "entity/moth.entity.json",
                json!({"format_version":"1.10.0","minecraft:client_entity":{
                "description":{"identifier":"example:moth",
                    "materials":{"default":"example:glow"},
                    "textures":{"default":"textures/entity/moth"},
                    "geometry":{"default":"geometry.moth"},
                    "render_controllers":["controller.render.moth"]}}})
                .to_string()
                .into_bytes(),
            ),
            (
                "models/entity/moth.geo.json",
                json!({"format_version":"1.12.0","minecraft:geometry":[{
                "description":{"identifier":"geometry.moth","texture_width":16,"texture_height":16},
                "bones":[{"name":"body","cubes":[{"origin":[0,0,0],"size":[4,4,4],"uv":[0,0]}]}]}]})
                .to_string()
                .into_bytes(),
            ),
            (
                "render_controllers/moth.json",
                json!({"format_version":"1.8.0","render_controllers":{
                "controller.render.moth":{"geometry":"Geometry.default",
                    "materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}})
                .to_string()
                .into_bytes(),
            ),
            (
                "materials/entity.material",
                json!({"materials":{"version":"1.0.0",
                "example:glow:entity_alphatest":{"+defines":["USE_EMISSIVE","FANCY"]}}})
                .to_string()
                .into_bytes(),
            ),
            ("textures/entity/moth.png", png),
        ];
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in files {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        let view = resource_pack::LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(vec![
                protocol::ResourcePackArchive::unencrypted(
                    id.parse().unwrap(),
                    "1.0.0".into(),
                    String::new(),
                    zip.finish().unwrap().into_inner(),
                ),
            ]),
        ));
        pack_runtime::entity_pack::compile(&view, None, None, None).expect("the pack compiles")
    }

    #[test]
    fn the_inspector_explains_each_published_entity_material() {
        let mut world = World::new();
        let runtime = Arc::new(assets::RuntimeAssets::diagnostic());
        world.insert_resource(ClientWorld::new(Arc::clone(&runtime)));
        assert_eq!(entity_materials(&world)["server_pack"], false);
        let mut client = ClientWorld::new(runtime);
        client.pack_entities = Some(pack());
        world.insert_resource(client);
        let report = entity_materials(&world);
        let glow = report["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|material| material["name"] == "example:glow")
            .expect("the drawn material is listed");
        assert_eq!(glow["source"], "materials/entity.material");
        assert_eq!(
            glow["chain"],
            json!(["example:glow", "entity_alphatest (client base)"])
        );
        assert_eq!(glow["variant"], "base");
        assert!(
            glow["applied_defines"]
                .as_array()
                .unwrap()
                .contains(&json!("USE_EMISSIVE"))
        );
        assert_eq!(glow["carried_defines"], json!(["FANCY"]));
        assert_eq!(glow["texture_slots"], json!([0]));
        assert!(glow["render_state"].as_str().is_some());
        assert_eq!(glow["reason"], "defines not drawn: FANCY");
        let text = report.to_string();
        assert!(!text.contains("rgba8"), "no pixel payloads");
    }

    #[test]
    fn the_block_inspector_lists_only_overlay_materials() {
        let mut world = World::new();
        world.insert_resource(ClientWorld::new(Arc::new(
            assets::RuntimeAssets::diagnostic(),
        )));
        let report = block_materials(&world);
        assert_eq!(report["available"], true);
        assert_eq!(report["total"], 0, "base-carrier materials are not listed");
    }
}
