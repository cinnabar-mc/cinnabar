//! Live attachables retain their own controller visibility and animated bones.
use super::*;

/// Supplies two independently animated cubes on a bound item root.
fn animated_item() -> Vec<(Box<str>, Vec<u8>)> {
    [
        ("attachables/tool.json", serde_json::json!({"format_version":"1.10.0","minecraft:attachable":{"description":{
            "identifier":"fixture:tool", "materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/entity/fixture"}, "geometry":{"default":"geometry.tool"},
            "animations":{"pose":"animation.tool"}, "scripts":{"animate":["pose"]},
            "render_controllers":[{"controller.render.tool":"query.is_in_ui && (context.owning_entity->query.mark_variant) != 4"}]
        }}})),
        ("models/tool.json", serde_json::json!({"format_version":"1.16.0","minecraft:geometry":[{
            "description":{"identifier":"geometry.tool","texture_width":16,"texture_height":16},
            "bones":[{"name":"root","binding":"q.item_slot_to_bone_name(c.item_slot)","pivot":[0,0,0],
                "cubes":[{"origin":[0,0,0],"size":[3,3,3],"uv":[0,0]}]},
                {"name":"tip","parent":"root","pivot":[0,4,0],
                "cubes":[{"origin":[0,4,0],"size":[2,2,2],"uv":[0,0]}]}]
        }]})),
        ("animations/tool.json", serde_json::json!({"format_version":"1.8.0","animations":{"animation.tool":{
            "loop":true,"bones":{"tip":{"rotation":[0,0,"query.life_time * 60"]}}
        }}})),
        ("render_controllers/tool.json", serde_json::json!({"format_version":"1.8.0","render_controllers":{"controller.render.tool":{
            "geometry":"Geometry.default", "materials":[{"*":"Material.default"}], "textures":["Texture.default"]
        }}})),
    ].into_iter().map(|(path, value)| (path.into(), serde_json::to_vec(&value).unwrap())).collect()
}

#[test]
fn live_ui_evaluates_multibone_attachable_and_its_ui_controller() {
    let pack = pack_with_files(false, animated_item());
    let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
    equip(&mut stream, Some("fixture:tool"));
    let mut ui = presentation(Arc::clone(&pack));
    ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
    ui.set_preview_pack_equipment(pack.equipment.clone());
    ui.set_player_preview_gear([None; 4], Some(("fixture:tool", 0)));
    let (_, before) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(triangles(&before), 24, "both animated item bones draw");
    let before_bones = &ui.gui_models.live_equipment.hands[0].as_ref().unwrap()[0]
        .model
        .transforms;
    let before_relative = before_bones[0].inverse() * before_bones[1];
    stream.advance_actor_interpolation_ticks(8);
    let (_, after) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
    assert_eq!(triangles(&after), 24);
    assert_ne!(before, after, "the live item must animate");
    let after_bones = &ui.gui_models.live_equipment.hands[0].as_ref().unwrap()[0]
        .model
        .transforms;
    assert_ne!(
        before_relative,
        after_bones[0].inverse() * after_bones[1],
        "the child animates independently of the player hand"
    );
    variant(&mut stream, 2, 4);
    assert_eq!(
        stream
            .authority()
            .actor(1)
            .unwrap()
            .metadata
            .get(&protocol_ids::ActorDataKey::MarkVariant.wire_value()),
        Some(&protocol::ActorMetadataValue::Int(4))
    );
    let hidden = draw(&mut ui, &stream, PreviewView::default());
    assert!(
        hidden
            .as_ref()
            .is_none_or(|(_, vertices)| triangles(vertices) == 0),
        "inactive draw: triangles {:?}, item layers {:?}",
        hidden.as_ref().map(|(_, v)| triangles(v)),
        ui.gui_models.live_equipment.hands[0].as_ref().map(Vec::len)
    );
    variant(&mut stream, 3, 0);
    assert_eq!(
        triangles(&draw(&mut ui, &stream, PreviewView::default()).unwrap().1),
        24
    );
}

/// Rewrites an authored fixture document before compilation, preserving the runtime path.
fn edit_item(
    files: &mut [(Box<str>, Vec<u8>)],
    path: &str,
    edit: impl FnOnce(&mut serde_json::Value),
) {
    let (_, bytes) = files
        .iter_mut()
        .find(|(name, _)| name.as_ref() == path)
        .unwrap();
    let mut document = serde_json::from_slice(bytes).unwrap();
    edit(&mut document);
    *bytes = serde_json::to_vec(&document).unwrap();
}

#[test]
fn live_attachable_selection_and_animation_inherit_ui_owner_variables() {
    // The player assigns variable.show_item = query.is_in_ui in pre_animation.
    for selection_gate in [false, true] {
        let mut files = animated_item();
        edit_item(&mut files, "attachables/tool.json", |document| {
            let description = &mut document["minecraft:attachable"]["description"];
            if selection_gate {
                description["item"] = serde_json::json!({"fixture:tool":"variable.show_item"});
            }
            description["render_controllers"] =
                serde_json::json!([{"controller.render.tool":"variable.show_item"}]);
        });
        let pack = pack_with_files(false, files);
        let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
        equip(&mut stream, Some("fixture:tool"));
        let mut ui = presentation(Arc::clone(&pack));
        ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
        ui.set_preview_pack_equipment(pack.equipment.clone());
        ui.set_player_preview_gear([None; 4], Some(("fixture:tool", 0)));
        let (_, vertices) = draw(&mut ui, &stream, PreviewView::default()).unwrap();
        assert_eq!(
            triangles(&vertices),
            24,
            "UI variable admits both animated item bones; selection gate {selection_gate}"
        );
        assert_eq!(
            ui.gui_models.live_equipment.hands[0]
                .as_ref()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn failed_live_attachable_falls_back_but_inactive_controller_stays_empty() {
    for failure in ["artwork", "evaluation", "inactive"] {
        let mut files = animated_item();
        edit_item(&mut files, "attachables/tool.json", |document| {
            let description = &mut document["minecraft:attachable"]["description"];
            description["textures"]["missing"] = "textures/entity/missing".into();
            if failure == "evaluation" {
                description["scripts"]["pre_animation"] =
                    serde_json::json!(["loop(10000, { variable.work = variable.work + 1; });"]);
            }
            if failure == "inactive" {
                description["render_controllers"] =
                    serde_json::json!([{"controller.render.tool":"0"}]);
            }
        });
        if failure == "artwork" {
            edit_item(&mut files, "render_controllers/tool.json", |document| {
                document["render_controllers"]["controller.render.tool"]["textures"] =
                    serde_json::json!(["query.is_in_ui ? Texture.missing : Texture.default"]);
            });
        }
        let pack = pack_with_files(false, files);
        let mut stream = tests::player_stream(Arc::clone(&pack.assets), Arc::clone(&pack.assets));
        equip(&mut stream, Some("fixture:tool"));
        let mut ui = presentation(Arc::clone(&pack));
        // The ordinary held cube is still available if the admitted attachable cannot evaluate.
        let fallback = ui
            .gui_models
            .sources
            .held
            .remove("minecraft:stone")
            .unwrap();
        ui.gui_models
            .sources
            .held
            .insert("fixture:tool".into(), fallback);
        ui.set_preview_equipment_assets(None, Some(Arc::clone(&pack.assets)));
        ui.set_preview_pack_equipment(pack.equipment.clone());
        ui.set_player_preview_gear([None; 4], Some(("fixture:tool", 0)));
        let drawn = draw(&mut ui, &stream, PreviewView::default());
        let layers = &ui.gui_models.live_equipment.hands[0];
        if failure == "inactive" {
            assert!(layers.as_ref().is_some_and(Vec::is_empty));
            assert!(drawn.is_none_or(|(_, vertices)| triangles(&vertices) == 0));
        } else {
            assert!(
                layers.is_none(),
                "{failure} must allow the ordinary held model"
            );
            assert_eq!(triangles(&drawn.unwrap().1), 12, "{failure} fallback cube");
        }
    }
}
