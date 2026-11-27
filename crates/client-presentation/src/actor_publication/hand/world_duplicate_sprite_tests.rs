//! Synthetic world-publication regression for ticket 11.
//!
//! The three frames in the report only show an unlabeled silhouette; the trace proves a
//! reachable path, not the real pack's identity. This fixture is therefore explicitly
//! synthetic: it authors a player render controller that draws a distinct 3D weapon model and
//! pairs it with a held attachable that is admitted but resolves no base artwork. It does NOT
//! assume that any controller layer or any admitted/no-draw definition owns a hand by itself;
//! the 3D layer is identified independently by its authored geometry and texture, and the
//! ordinary sprite is tied to the single local actor's single held item.

use super::*;
use crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE;
use crate::presentation::equipment::LAYER_MAIN_HAND;
use bevy::ecs::system::SystemState;
use bevy::prelude::{PerspectiveProjection, Projection, Time, Transform, World};
use bevy::time::Real;
use render_model::ActorArtworkPages;
use serde_json::json;

const HELD: &str = "minecraft:shield";
const WEAPON_GEOMETRY: &str = "geometry.fixture_held_weapon";
const WEAPON_TEXTURE: &str = "textures/fixture_held_weapon";
const UNRELATED_GEOMETRY: &str = "geometry.fixture_unrelated";
const UNRELATED_TEXTURE: &str = "textures/fixture_unrelated";
const MISSING_BASE: &str = "textures/missing_held_base";
const HELD_ITEM_VARIABLE: &str = "variable.fixture_held_item";

/// The authored name TACZ-style packs compare in their pre_animation script: item names are
/// legacy names without the namespace, so `minecraft:shield` reads as `shield`.
fn held_item_name() -> &'static str {
    HELD.split_once(':').map_or(HELD, |(_, name)| name)
}

fn edit_json(
    files: &mut [(Box<str>, Vec<u8>)],
    path: &str,
    edit: impl FnOnce(&mut serde_json::Value),
) {
    let (_, bytes) = files
        .iter_mut()
        .find(|(name, _)| name.as_ref() == path)
        .unwrap();
    let mut value = serde_json::from_slice(bytes).unwrap();
    edit(&mut value);
    *bytes = serde_json::to_vec(&value).unwrap();
}

/// Authors the TACZ-style item variable: a script variable assigned from the equipped
/// main-hand item name. The weapon controller below activates on that variable, so its 3D
/// geometry is tied to the same item the ordinary sprite would draw.
fn add_item_gated_pre_animation(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    edit_json(files, "entity/player.json", |value| {
        value["minecraft:client_entity"]["description"]["scripts"]["pre_animation"] =
            json!([format!(
                "{HELD_ITEM_VARIABLE} = query.get_equipped_item_name == '{}';",
                held_item_name()
            )]);
    });
}

/// Adds a player render controller whose separate geometry stands in for the weapon the
/// report shows on the player model. Its activation is authored through the item variable,
/// not an unconditional `true`: the 3D draw is tied to the held item. It is deliberately a
/// different geometry and texture from both the body and the item icon so the draw can be
/// identified on its own.
fn add_weapon_controller(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    add_separate_controller(
        files,
        "weapon",
        WEAPON_GEOMETRY,
        WEAPON_TEXTURE,
        "fixture_weapon",
        "fixture_held_weapon",
        HELD_ITEM_VARIABLE,
    );
}

/// Adds a separate player-controller geometry with the same authorship shape as the weapon
/// fixture. The negative control below uses it to show that an extra controller model alone
/// carries no main-hand ownership information the world publication can read.
fn add_unrelated_controller(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    add_separate_controller(
        files,
        "extra",
        UNRELATED_GEOMETRY,
        UNRELATED_TEXTURE,
        "fixture_unrelated",
        "fixture_unrelated",
        "true",
    );
}

/// A controller gated on the *offhand* item with the same authored shape as the weapon: it
/// must not be mistaken for a main-hand replacement when the offhand happens to hold that item.
fn add_offhand_controller(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    add_separate_controller(
        files,
        "offhand",
        UNRELATED_GEOMETRY,
        UNRELATED_TEXTURE,
        "fixture_offhand",
        "fixture_offhand",
        &format!(
            "query.get_equipped_item_name('off_hand') == '{}'",
            held_item_name()
        ),
    );
}

/// Samples a valid extra model only when a different full item identifier is held. The held
/// shield shares neither its identifier nor the authored suffix of this gate.
fn add_wrong_held_controller(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    add_separate_controller(
        files,
        "wrong_held",
        UNRELATED_GEOMETRY,
        UNRELATED_TEXTURE,
        "fixture_wrong_held",
        "fixture_wrong_held",
        &format!("{HELD_ITEM_VARIABLE} == 'diamond_sword'"),
    );
}

fn add_separate_controller(
    files: &mut Vec<(Box<str>, Vec<u8>)>,
    alias: &str,
    geometry: &str,
    texture: &str,
    stem: &str,
    file_stem: &str,
    condition: &str,
) {
    let animation = format!("animation.{stem}.pose");
    let controller = format!("controller.render.{stem}");
    edit_json(files, "entity/player.json", |value| {
        let description = &mut value["minecraft:client_entity"]["description"];
        description["geometry"][alias] = geometry.into();
        description["textures"][alias] = texture.into();
        description["materials"][alias] = "entity_alphablend".into();
        description["animations"][alias] = animation.clone().into();
        description["scripts"]["animate"]
            .as_array_mut()
            .unwrap()
            .push(json!(alias));
        description["render_controllers"]
            .as_array_mut()
            .unwrap()
            .push(json!({(controller.clone()): condition}));
    });
    edit_json(files, "render_controllers/test.json", |value| {
        value["render_controllers"][controller] = json!({
            "geometry": format!("Geometry.{alias}"),
            "textures": [format!("Texture.{alias}")],
            "materials": [{"*": format!("Material.{alias}")}],
        });
    });
    let bone = format!("{alias}_root");
    files.push((
        format!("models/entity/{file_stem}.geo.json").into(),
        serde_json::to_vec(&json!({
            "format_version": "1.12.0",
            "minecraft:geometry": [{
                "description": {
                    "identifier": geometry,
                    "texture_width": 16, "texture_height": 16
                },
                "bones": [{
                    "name": bone, "pivot": [0, 0, 0],
                    "cubes": [{"origin": [0, 20, -4], "size": [2, 2, 4], "uv": [0, 0]}]
                }]
            }]
        }))
        .unwrap(),
    ));
    files.push((
        format!("animations/{file_stem}.animation.json").into(),
        serde_json::to_vec(&json!({
            "format_version": "1.8.0",
            "animations": {animation: {
                "loop": true, "bones": {bone: {"rotation": [0, 35, 0]}}
            }}
        }))
        .unwrap(),
    ));
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(16, 16, image::Rgba([224, 32, 64, 192]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    files.push((format!("textures/{file_stem}.png").into(), png.into_inner()));
}

/// Keeps the shield attachable's geometry, controller and alias authored, but makes the alias
/// name a PNG the compiler does not ship, so the attachable is admitted yet draws no artwork.
fn admit_without_draw(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    edit_json(files, "attachables/shield.json", |value| {
        value["minecraft:attachable"]["description"]["textures"]["default"] = json!(MISSING_BASE);
    });
}

/// Gives the admitted held attachable a world-only authored zero-scale root pose. This is
/// separate from the player's independently submitted weapon controller model above.
fn author_zero_world_pose(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    edit_json(files, "attachables/shield.json", |value| {
        let description = &mut value["minecraft:attachable"]["description"];
        description["animations"]["zero_world"] = "animation.fixture.zero_world".into();
        description["scripts"]["animate"] = json!(["zero_world"]);
    });
    files.push((
        "animations/zero_world.animation.json".into(),
        serde_json::to_vec(&json!({
            "format_version": "1.8.0",
            "animations": {"animation.fixture.zero_world": {
                "loop": true,
                "bones": {"item": {"scale": [0, 0, 0]}}
            }}
        }))
        .unwrap(),
    ));
}

fn assert_authored_zero_world_pose(entities: &assets::RuntimeEntityAssets) {
    let symbol = entities
        .symbols()
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "animation.fixture.zero_world")
        .expect("the zero-world animation is compiled");
    let clip = entities
        .animation_clips()
        .iter()
        .find(|clip| clip.symbol == symbol as u32)
        .expect("the attachable's authored zero-world clip is compiled");
    let channel = entities
        .animation_channels()
        .get(clip.first_channel as usize..(clip.first_channel + clip.channel_count) as usize)
        .and_then(|channels| {
            channels.iter().find(|channel| {
                channel.bone_name.as_deref() == Some("item")
                    && channel.property == assets::EntityAnimationProperty::Scale
            })
        })
        .expect("the compiled clip scales the actual attachable root bone");
    let frame = entities
        .animation_keyframes()
        .get(
            channel.first_keyframe as usize
                ..(channel.first_keyframe + channel.keyframe_count) as usize,
        )
        .and_then(|frames| frames.first())
        .expect("the compiled root scale has a keyframe");
    assert_eq!(frame.value.map(|value| value.get()), [0.0; 3]);
    assert_eq!(frame.expressions, [None; 3]);
}

/// Binds the attachable to a different item so no definition is admitted for the held shield.
fn unbind(files: &mut Vec<(Box<str>, Vec<u8>)>) {
    edit_json(files, "attachables/shield.json", |value| {
        value["minecraft:attachable"]["description"]["identifier"] =
            json!("minecraft:shield_unbound");
    });
}

type Fixture = (
    (
        WorldStream,
        EquipmentRuntime,
        ActorArtworkPages,
        ActorEquipmentInput,
    ),
    Arc<assets::RuntimeEntityAssets>,
);

fn fixture(edit_files: impl FnOnce(&mut Vec<(Box<str>, Vec<u8>)>)) -> Fixture {
    native_tests::fixture_with_files(
        Some(HELD),
        false,
        false,
        true,
        false,
        false,
        None,
        edit_files,
    )
}

/// Runs the real early and final world-publication owners with a local third-person camera.
/// Returns the captures behind `PreparedActorPublication::submissions()`.
fn publish_world(
    mut stream: WorldStream,
    equipment: EquipmentRuntime,
    artwork: ActorArtworkPages,
    input: ActorEquipmentInput,
    entities: Arc<assets::RuntimeEntityAssets>,
    icon_fallback: bool,
) -> Vec<render_model::ActorRigSubmission> {
    let mut world = World::new();
    let mut time = Time::<Real>::default();
    time.advance_by(std::time::Duration::from_millis(50));
    world.insert_resource(time);
    world.insert_resource(equipment);
    world.insert_resource(render_scene::ActorArtworkResource(artwork));
    world.insert_resource(
        render_scene::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap(),
    );
    world.insert_resource(super::super::HandRigBuilder::from_runtime_assets(&entities).unwrap());
    world.init_resource::<super::super::ActorFrameState>();
    world.init_resource::<super::super::PreparedActorPublication>();
    world.init_resource::<super::super::ActorFramePartialTick>();
    world.init_resource::<render_scene::HandRigScene>();
    let mut avatar = crate::local_player::LocalAvatarPresentation::default();
    avatar.begin_session(
        simulation_clock::identity::SessionGeneration::new(stream.authority().actor_session_id()),
        1,
    );
    world.insert_resource(avatar);
    world.init_resource::<crate::local_player::LocalAvatarVisibilityCarrier>();
    let mut settings = crate::camera::CameraSettingsAuthority::default();
    let mut user = ui::UserSettings::default();
    user.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
    user.video.java_animations = false;
    // The production frame reads this setting every tick and overwrites any value set directly
    // on the runtime, so an icon-fallback fixture must disable it through settings.
    user.video.show_custom_item_models = !icon_fallback;
    settings.replace(1, &user).unwrap();
    world.insert_resource(settings);
    world.insert_resource(crate::local_player::LocalViewPose::default());
    world.spawn((
        Transform::default(),
        Projection::Perspective(PerspectiveProjection::default()),
        crate::camera::FlyCamera::default(),
    ));
    if icon_fallback {
        world
            .resource_mut::<EquipmentRuntime>()
            .set_show_custom_item_models(false);
    }
    let mut params = SystemState::<super::super::ActorFramePublication>::new(&mut world);
    let mut prepared_artwork = None;
    super::super::advance_actor_frame(
        super::super::ActorWorld {
            stream: Some(&mut stream),
            collisions: None,
            entity_assets: Some(&entities),
            pack_entities: None,
            session_items: None,
            prepared_actor_artwork: &mut prepared_artwork,
        },
        super::super::ActorFrameInput {
            cosmetics: Vec::new(),
            local_feed: None,
            predicted_eye: Some([0.0, 65.62, 0.0]),
            predicted_feet: Some([0.0, 64.0, 0.0]),
            local_equipment: input,
            swing_progress: None,
            renders_game: true,
            hide_hand: false,
            remote_emotes: Vec::new(),
            remote_hats: Vec::new(),
            custom_emote: None,
        },
        |_| {},
        |_, _| (None, None),
        params.get_mut(&mut world).unwrap(),
    );
    world
        .resource_mut::<super::super::ActorFrameState>()
        .step
        .as_mut()
        .unwrap()
        .partial_tick = 0.5;
    super::super::prepare_actor_render_frame(
        super::super::ActorWorld {
            stream: Some(&mut stream),
            collisions: None,
            entity_assets: Some(&entities),
            pack_entities: None,
            session_items: None,
            prepared_actor_artwork: &mut prepared_artwork,
        },
        None,
        |_, _, _, _| false,
        None,
        params.get_mut(&mut world).unwrap(),
    );
    world
        .resource::<super::super::PreparedActorPublication>()
        .submissions()
        .expect("the final owner prepared a world batch")
        .to_vec()
}

/// Identifies the authored 3D weapon layer from the sampled controller output, independently of
/// equipment: its geometry is the weapon geometry and its texture differs from the body's.
fn sampled_weapon_rig(
    stream: &WorldStream,
    artwork: &ActorArtworkPages,
    entities: &assets::RuntimeEntityAssets,
) -> render_model::EntityRigId {
    let mut frame = stream.authority().actor_render_frame(1.0);
    let layers = frame
        .layers(1)
        .expect("the local player has sampled render controllers");
    let body = layers
        .iter()
        .find(|layer| layer.geometry.is_none())
        .expect("the base body layer remains");
    let weapon = layers
        .iter()
        .find(|layer| layer.geometry.is_some())
        .expect("the authored controller selects a separate 3D weapon geometry");
    let geometry = weapon.geometry.unwrap();
    assert_eq!(
        entities
            .geometries()
            .get(geometry as usize)
            .expect("the sampled geometry is compiled")
            .identifier
            .as_ref(),
        WEAPON_GEOMETRY,
        "the 3D layer is the authored weapon model, not an unrelated controller"
    );
    assert_ne!(
        artwork
            .source_location(weapon.pack, weapon.source)
            .expect("the weapon texture is compiled"),
        artwork
            .source_location(body.pack, body.source)
            .expect("the body texture is compiled"),
        "the weapon model has its own texture"
    );
    let catalog_rig = if weapon.pack {
        render_model::pack_rig_id(0)
    } else {
        render_model::EntityRigId(0)
    };
    render_model::layer_geometry_rig_id(catalog_rig, geometry)
}

/// Asserts the compiled catalog really admits an attachable definition for the held shield and
/// that its base alias names an absent PNG: the `admitted_without_draw` hypothesis from the
/// trace, not a claim that this definition alone owns the hand.
fn assert_admitted_without_draw(entities: &assets::RuntimeEntityAssets) {
    let symbol = entities
        .symbols()
        .iter()
        .find(|symbol| symbol.identifier.as_ref() == HELD)
        .expect("the held shield is compiled");
    assert_eq!(symbol.kind, assets::EntityAssetKind::Attachable);
    assert!(
        entities.attachable_rig_binding(HELD).is_some(),
        "a real attachable definition is admitted for the held shield"
    );
    let resolved = |kind: assets::EntityDependencyKind, identifier: &str| {
        symbol.dependencies.iter().any(|dependency| {
            dependency.kind == kind
                && dependency.identifier.as_ref() == identifier
                && dependency.resolution == assets::EntityDependencyResolution::Catalog
        })
    };
    assert!(
        resolved(assets::EntityDependencyKind::Geometry, "geometry.item_test")
            && resolved(
                assets::EntityDependencyKind::RenderController,
                "controller.render.test"
            ),
        "the admitted definition keeps valid geometry and a render controller"
    );
    assert!(
        symbol.dependencies.iter().any(|dependency| {
            dependency.kind == assets::EntityDependencyKind::Texture
                && dependency.identifier.as_ref() == MISSING_BASE
                && dependency.resolution == assets::EntityDependencyResolution::External
        }),
        "the base alias is authored but its PNG is absent, so the attachable draws nothing"
    );
}

fn main_hand_sprites(
    submissions: &[render_model::ActorRigSubmission],
) -> Vec<&render_model::ActorRigSubmission> {
    // One synthetic actor (runtime 1) and one held item (`input.main` only): the main-hand
    // layer for that runtime is the shield's ordinary sprite, not an arbitrary layer count.
    submissions
        .iter()
        .filter(|draw| {
            draw.input.identity.runtime_id == 1
                && draw.input.identity.layer == LAYER_MAIN_HAND
                && draw.input.current_bones.len() == 1
        })
        .collect()
}

#[test]
fn synthetic_world_admitted_no_draw_with_3d_weapon_has_no_duplicate_main_hand_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(|files| {
        add_item_gated_pre_animation(files);
        add_weapon_controller(files);
        author_zero_world_pose(files);
        admit_without_draw(files);
    });
    assert_eq!(
        input.main.as_ref().unwrap().identifier.as_ref(),
        HELD,
        "the synthetic world holds only the shield"
    );
    assert_admitted_without_draw(&entities);
    assert_authored_zero_world_pose(&entities);
    let weapon_rig = sampled_weapon_rig(&stream, &artwork, &entities);

    let submissions = publish_world(stream, equipment, artwork, input, entities, false);

    let three_d = submissions
        .iter()
        .find(|draw| {
            draw.input.identity.runtime_id == 1
                && draw.input.rig == weapon_rig
                && draw.input.identity.layer >= ACTOR_LAYER_TEXTURE_BASE
        })
        .expect("the player's 3D weapon layer remains in world publication");
    assert_ne!(
        three_d.route,
        render_model::ActorRigRoute::NoDraw,
        "the identified 3D weapon layer is actually drawn"
    );

    // RED: production 563c28ab suppresses this only for the first-person hand path, so the
    // world batch still carries an ordinary main-hand sprite beside the 3D weapon.
    let duplicates = main_hand_sprites(&submissions);
    if let Some(sprite) = duplicates.first() {
        assert!(
            !render_model::is_layer_geometry_rig_id(sprite.input.rig),
            "the extra main-hand draw is an ordinary item sprite, not the 3D weapon layer"
        );
    }
    assert!(
        duplicates.is_empty(),
        "a 3D weapon model must not keep a duplicate ordinary main-hand sprite in the world: {:?}",
        duplicates
            .iter()
            .map(|draw| (draw.input.identity.layer, draw.input.rig))
            .collect::<Vec<_>>()
    );
}

#[test]
fn synthetic_world_unbound_item_without_3d_keeps_ordinary_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(unbind);
    assert!(
        entities.attachable_rig_binding(HELD).is_none(),
        "the control admits no definition for the held shield"
    );
    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "an unbound item with no 3D replacement keeps its ordinary sprite"
    );
}

#[test]
fn synthetic_world_admitted_no_draw_without_3d_keeps_ordinary_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(admit_without_draw);
    assert_admitted_without_draw(&entities);
    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "an admitted no-draw definition alone does not remove the ordinary sprite when no 3D layer replaces it"
    );
}

#[test]
fn synthetic_world_geometry_only_startup_fallback_keeps_ordinary_sprite() {
    const SWORD: &str = "minecraft:diamond_sword";
    const EXTERNAL: &str = "geometry.startup_fallback_external";
    let ((stream, equipment, artwork, input), entities, catalog) =
        native_tests::fixture_with_catalog(
            Some(SWORD),
            false,
            false,
            true,
            false,
            false,
            None,
            |_| {},
            |bindings| {
                let mut binding = bindings
                    .iter()
                    .find(|binding| binding.identifier.as_ref() == HELD)
                    .unwrap()
                    .clone();
                binding.identifier = SWORD.into();
                binding.attachable = None;
                binding.geometry.identifier = EXTERNAL.into();
                binding.geometry.resolution = assets::EntityDependencyResolution::External;
                bindings.push(binding);
                bindings.sort_by(|left, right| left.identifier.cmp(&right.identifier));
            },
        );
    let binding = catalog
        .bindings_for(SWORD)
        .iter()
        .find(|binding| {
            binding.attachable.is_none()
                && binding.geometry.resolution == assets::EntityDependencyResolution::External
                && binding.geometry.identifier.as_ref() == EXTERNAL
        })
        .expect("the startup catalog admits the external geometry-only binding");
    assert!(
        entities
            .attachable_rig_binding(binding.attachable_identifier())
            .is_none(),
        "no authored definition can evaluate for the geometry-only fallback"
    );
    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "a geometry-only startup fallback keeps its ordinary sprite in the world"
    );
}

#[test]
fn synthetic_world_explicit_icon_fallback_keeps_ordinary_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(|files| {
        add_item_gated_pre_animation(files);
        add_weapon_controller(files);
        admit_without_draw(files);
    });
    assert_admitted_without_draw(&entities);
    let submissions = publish_world(stream, equipment, artwork, input, entities, true);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "disabling custom item models keeps the explicit icon fallback sprite"
    );
}

/// Negative control for the missing ownership signal: this player controller has the same
/// authorship shape as the weapon fixture — always active, its own geometry and texture — but
/// is not a replacement for the held item. Today's publication sees only an admitted/no-draw
/// definition plus one extra controller geometry in both fixtures, so the two cases cannot be
/// distinguished from the contract available to `push_held`. Any fix that suppresses the sprite
/// from that combination alone breaks this control.
#[test]
fn synthetic_world_unrelated_controller_keeps_ordinary_sprite() {
    let ((stream, equipment, artwork, input), entities, catalog) =
        native_tests::fixture_with_catalog(
            Some(HELD),
            false,
            false,
            true,
            false,
            false,
            None,
            |files| {
                add_item_gated_pre_animation(files);
                add_unrelated_controller(files);
                admit_without_draw(files);
            },
            |bindings| {
                let mut other_namespace = bindings
                    .iter()
                    .find(|binding| binding.identifier.as_ref() == HELD)
                    .unwrap()
                    .clone();
                other_namespace.identifier = "fixturemod:shield".into();
                bindings.push(other_namespace);
                bindings.sort_by(|left, right| left.identifier.cmp(&right.identifier));
            },
        );
    assert_admitted_without_draw(&entities);
    let same_suffix = catalog.bindings_for("fixturemod:shield");
    assert!(
        same_suffix.iter().any(|binding| {
            binding.identifier.as_ref() == "fixturemod:shield"
                && binding.geometry.resolution == assets::EntityDependencyResolution::Catalog
        }),
        "the catalog admits a separate attachable binding with the same suffix in another namespace"
    );
    let mut frame = stream.authority().actor_render_frame(1.0);
    let layers = frame
        .layers(1)
        .expect("the player has sampled render controllers");
    let body = layers
        .iter()
        .find(|layer| layer.geometry.is_none())
        .expect("the sampled base-body layer remains active");
    let unrelated = layers
        .iter()
        .find(|layer| layer.geometry.is_some())
        .expect("the unrelated controller draws a separate geometry");
    assert!(
        unrelated.authored_layer.is_some(),
        "the active unrelated model has an authored layer index"
    );
    assert_ne!(
        unrelated.authored_layer, body.authored_layer,
        "the unrelated model is a distinct sampled layer, not an absent/inactive target"
    );
    assert_eq!(
        entities
            .geometries()
            .get(unrelated.geometry.unwrap() as usize)
            .expect("the unrelated geometry is compiled")
            .identifier
            .as_ref(),
        UNRELATED_GEOMETRY,
        "the negative control really is the unrelated geometry"
    );
    assert!(
        artwork
            .source_location(unrelated.pack, unrelated.source)
            .is_some(),
        "the sampled unrelated layer has valid compiled artwork"
    );

    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "an extra controller geometry with no authored slot/item link does not own the main hand"
    );
}

#[test]
fn synthetic_world_controller_for_wrong_full_held_identifier_keeps_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(|files| {
        add_item_gated_pre_animation(files);
        add_wrong_held_controller(files);
        admit_without_draw(files);
    });
    assert_eq!(input.main.as_ref().unwrap().identifier.as_ref(), HELD);
    assert_admitted_without_draw(&entities);

    let mut frame = stream.authority().actor_render_frame(1.0);
    let layers = frame
        .layers(1)
        .expect("the player has sampled render controllers");
    assert!(
        layers.iter().any(|layer| layer.geometry.is_none()),
        "the base player layer is sampled"
    );
    assert!(
        layers.iter().all(|layer| layer.geometry.is_none()),
        "the controller gated on minecraft:diamond_sword is inactive for minecraft:shield"
    );
    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "a different full held identifier cannot suppress the shield sprite"
    );
}

/// The offhand is not the main hand: an offhand-item-gated controller must not suppress the
/// main-hand sprite even when the same authored variable name pattern is used.
#[test]
fn synthetic_world_offhand_gated_controller_keeps_main_hand_sprite() {
    let ((stream, equipment, artwork, input), entities) = fixture(|files| {
        add_offhand_controller(files);
        admit_without_draw(files);
    });
    assert_admitted_without_draw(&entities);
    let submissions = publish_world(stream, equipment, artwork, input, entities, false);
    assert!(
        !main_hand_sprites(&submissions).is_empty(),
        "an offhand-gated controller does not own the main hand"
    );
}
