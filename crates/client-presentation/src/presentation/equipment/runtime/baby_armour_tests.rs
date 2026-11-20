//! Baby wearers of mobs that opt in to dedicated baby armor draw the pack's baby armor model and
//! artwork; adults, and babies of mobs that never opt in, keep the adult pieces.

use super::{elytra_tests::selected_pixel, pack_runtime, player_body};
use crate::presentation::equipment::runtime::{
    ActorEquipmentInput, EquipmentAnimation, HeldKind, WornItem,
};
use client_world::WorldAuthority;
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap,
    WorldEvent,
};
use protocol_ids::ActorFlag;
use render_model::EntityRigId;
use std::sync::Arc;

const ADULT: [u8; 4] = [170, 170, 170, 255];
const BABY: [u8; 4] = [30, 150, 70, 255];
const MOB: u64 = 2;

/// An original solid-colour raster.
fn solid(rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(16, 16, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// One cube on a `head` bone, wider for larger `half` values.
fn head_geometry(identifier: &str, half: f32) -> Vec<u8> {
    let (low, width) = (-half, half * 2.0);
    serde_json::to_vec(&serde_json::json!({
        "format_version":"1.12.0","minecraft:geometry":[{
            "description":{"identifier":identifier,"texture_width":16,"texture_height":16},
            "bones":[{"name":"head","pivot":[0,24,0],
                "cubes":[{"origin":[low,24,low],"size":[width,8,width],"uv":[0,0]}]}]
        }]
    }))
    .unwrap()
}

/// Mobs that the world animates: a zombie, which sets the opt-in variable as the pinned zombie
/// does, and another mob that never does.
fn owner_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let entity = |identifier: &str, scripts: Option<serde_json::Value>| {
        let mut description = serde_json::json!({
            "identifier":identifier,"materials":{"default":"entity_alphatest"},
            "textures":{"default":"textures/fixture_mob"},
            "geometry":{"default":"geometry.fixture_mob"},
            "render_controllers":["controller.render.mob"]
        });
        if let Some(scripts) = scripts {
            description["scripts"] = scripts;
        }
        (
            Box::<str>::from(format!("entity/{}.json", identifier.replace(':', "_"))),
            serde_json::to_vec(&serde_json::json!({
                "format_version":"1.10.0",
                "minecraft:client_entity":{"description":description}
            }))
            .unwrap(),
        )
    };
    vec![
        entity(
            "minecraft:zombie",
            Some(serde_json::json!({
                "pre_animation":["v.use_dedicated_baby_armor = true;"],
                "variables":{"variable.use_dedicated_baby_armor":"public"}
            })),
        ),
        entity("fixture:plain_mob", None),
        (
            "models/entity/fixture_mob.json".into(),
            head_geometry("geometry.fixture_mob", 4.0),
        ),
        (
            "render_controllers/mob.json".into(),
            br#"{"format_version":"1.8.0","render_controllers":{
                "controller.render.mob":{"geometry":"Geometry.default",
                    "materials":[{"*":"Material.default"}],"textures":["Texture.default"]}}}"#
                .to_vec(),
        ),
        ("textures/fixture_mob.png".into(), solid(ADULT)),
    ]
}

/// A stock-shaped iron helmet with its adult and baby resources, completed with the baby branch
/// the compiler adds to stock armour.
fn armour_pack() -> Vec<(Box<str>, Vec<u8>)> {
    let mut files: Vec<(Box<str>, Vec<u8>)> = vec![
        (
            "attachables/iron_helmet.json".into(),
            serde_json::to_vec(&serde_json::json!({
                "format_version":"1.8.0","minecraft:attachable":{"description":{
                    "identifier":"minecraft:iron_helmet",
                    "materials":{"default":"armor","enchanted":"armor_enchanted"},
                    "textures":{"default":"textures/models/armor/iron_1",
                        "enchanted":"textures/models/glint"},
                    "geometry":{"default":"geometry.humanoid.armor.helmet"},
                    "scripts":{"parent_setup":"variable.helmet_layer_visible = 0.0;"},
                    "render_controllers":["controller.render.armor"]
                }}
            }))
            .unwrap(),
        ),
        (
            "models/entity/armor_helmet.json".into(),
            head_geometry("geometry.humanoid.armor.helmet", 5.0),
        ),
        (
            "models/entity/baby_armor_helmet.geo.json".into(),
            head_geometry("geometry.humanoid.baby.armor.helmet", 3.0),
        ),
        (
            "render_controllers/armor.render_controllers.json".into(),
            br#"{"format_version":"1.8.0","render_controllers":{
                "controller.render.armor":{"geometry":"Geometry.default",
                    "materials":[{"*":"variable.is_enchanted ? Material.enchanted : Material.default"}],
                    "textures":["variable.has_trim ? variable.trim_path : Texture.default","Texture.enchanted"]}}}"#
                .to_vec(),
        ),
        (
            "render_controllers/armor.v2.render_controllers.json".into(),
            br#"{"format_version":"1.8.0","render_controllers":{
                "controller.render.armor.v2":{
                    "geometry":"variable.use_baby_geo ? Geometry.baby : Geometry.default",
                    "materials":[{"*":"variable.is_enchanted ? Material.enchanted : Material.default"}],
                    "textures":["variable.use_baby_geo ? Texture.baby : (variable.has_trim ? variable.trim_path : Texture.default)","Texture.enchanted"]}}}"#
                .to_vec(),
        ),
        ("textures/models/armor/iron_1.png".into(), solid(ADULT)),
        ("textures/models/armor/iron_baby.png".into(), solid(BABY)),
        (
            "textures/models/glint.png".into(),
            solid([255, 255, 255, 255]),
        ),
    ];
    let documents: Vec<serde_json::Value> = files
        .iter()
        .filter(|(path, _)| path.starts_with("attachables/"))
        .map(|(_, bytes)| serde_json::from_slice(bytes).unwrap())
        .collect();
    let baby = pack_entity_compiler::missing_baby_armor_bindings(
        |path| files.iter().any(|(source, _)| source.as_ref() == path),
        documents.iter(),
    )
    .unwrap();
    assert_eq!(baby.len(), 1, "the stock helmet gains one baby branch");
    files.extend(
        baby.into_iter()
            .map(|(path, document)| (path, serde_json::to_vec(&document).unwrap())),
    );
    files
}

/// Equips an iron helmet.
fn helmet() -> ActorEquipmentInput {
    ActorEquipmentInput {
        armor: [
            Some(WornItem {
                trim: None,
                variant: Default::default(),
                map_id: None,
                shield_pattern: None,
                charged_projectile: None,
                use_facts: client_world::EffectiveItemUse::default(),
                grip: assets::item::grip::builtin_item_grip("minecraft:iron_helmet"),
                identifier: Arc::from("minecraft:iron_helmet"),
                metadata: 0,
                damage: None,
                kind: HeldKind::Other,
                dye_rgb: None,
                enchanted: false,
                glint: Default::default(),
            }),
            None,
            None,
            None,
        ],
        ..Default::default()
    }
}

/// Animates one mob of `identifier` through the world's own scripts, dresses it in the iron helmet
/// and returns the first texel and model of the helmet that is drawn.
fn worn_helmet(identifier: &str, baby: bool) -> ([u8; 4], EntityRigId) {
    let compiled = pack_entity_compiler::compile_actor_pack(owner_pack())
        .unwrap()
        .unwrap();
    let entities = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap());
    let mut world = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Some(entities),
        [0.0; 3],
        None,
    );
    let metadata: Arc<[ActorMetadata]> = if baby {
        Arc::from([ActorMetadata {
            key: 0,
            value: ActorMetadataValue::Flags(1 << ActorFlag::Baby.wire_value()),
        }])
    } else {
        Arc::from([])
    };
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: MOB as i64,
                runtime_id: MOB,
                kind: ActorKind::Entity {
                    identifier: identifier.into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata,
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    test_time::eventually("mob appearance preparation", || {
        !world.poll_actor_appearance_preparation()
    });
    world.advance_actor_interpolation_ticks(1);
    let rig = world.actor_rig(MOB).expect("the mob has a sampled rig");
    let owner = world.actor(MOB).unwrap();
    let (mut runtime, pages) = pack_runtime(armour_pack());
    let body = player_body(&mut runtime);
    let drawn = runtime
        .layers_for(
            &body,
            &helmet(),
            Some(EquipmentAnimation {
                owner,
                rig: &rig,
                frame_alpha: 1.0,
                delta_seconds: simulation_clock::TICK_DURATION.as_secs_f32(),
            }),
        )
        .to_vec();
    assert_eq!(drawn.len(), 1, "{identifier} baby={baby}");
    (
        selected_pixel(&pages, &drawn[0]),
        drawn[0].submission.input.rig,
    )
}

#[test]
fn baby_wearers_of_dedicated_armor_mobs_draw_baby_armor() {
    let (adult_texel, adult_model) = worn_helmet("minecraft:zombie", false);
    assert_eq!(adult_texel, ADULT);
    let (baby_texel, baby_model) = worn_helmet("minecraft:zombie", true);
    assert_eq!(baby_texel, BABY, "a baby zombie wears the baby artwork");
    assert!(
        baby_model != adult_model,
        "a baby zombie wears the baby armor model, not a scaled adult one"
    );
}

#[test]
fn babies_of_mobs_without_the_opt_in_keep_adult_armor() {
    let (adult_texel, adult_model) = worn_helmet("minecraft:zombie", false);
    let (texel, model) = worn_helmet("fixture:plain_mob", true);
    assert_eq!(texel, adult_texel);
    assert!(model == adult_model);
}
