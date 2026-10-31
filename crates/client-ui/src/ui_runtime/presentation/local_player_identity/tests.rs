use super::*;
use crate::{
    test_support::{mini_engine_presentation, rasterize},
    ui_runtime::UiRuntime,
};
use protocol::PlayerSkin;
use serde_json::json;
use sha2::{Digest, Sha256};
use ui::mod_panel::local_player_head as identity;
use ui::{
    DpiScale,
    mod_panel::{Surface, SurfaceValue},
};

const FACE: [u8; 4] = [31, 73, 113, 255];
const HAT: [u8; 4] = [211, 137, 43, 255];
const SECOND: [u8; 4] = [97, 181, 229, 255];
const NEUTRAL: [u8; 4] = [0, 255, 255, 255];

/// Original face texels with one opaque hat texel to exercise layered extraction.
fn skin(color: [u8; 4]) -> StandardSkin {
    let side = render_api::CLASSIC_SKIN_SIDE;
    let mut rgba = vec![0; side * side * 4];
    for y in 8..16 {
        for x in 8..16 {
            let at = (y * side + x) * 4;
            rgba[at..at + 4].copy_from_slice(&color);
        }
    }
    let at = (8 * side + 40) * 4;
    rgba[at..at + 4].copy_from_slice(&HAT);
    StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: rgba.into(),
        cape: None,
        geometry: None,
    }
}

/// A live local actor's appearance feed, without carriers or network traffic.
fn feed(name: &str, skin: PlayerSkin) -> client_world::LocalPlayerFeed {
    client_world::LocalPlayerFeed {
        game_mode: None,
        uuid: [5; 16],
        username: name.into(),
        skin,
        prefer_client_skin: true,
        position: [0.0; 3],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        swimming: false,
        spinning: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        equipment_count: 0,
        held_network_ids: [0; 2],
        held_variants: Default::default(),
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

/// A fresh world's local identity, including its own runtime actor id.
fn profile_world(actor: u64, name: &str, skin: PlayerSkin) -> client_world::WorldAuthority {
    let mut world = client_world::WorldAuthority::new(
        protocol::WorldBootstrap {
            local_player_unique_id: actor as i64,
            local_player_runtime_id: actor,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    world.sync_local_player_pose(&feed(name, skin));
    world
}

/// An authored portrait, native name and original neutral fallback artwork.
fn identity_surface() -> Surface {
    Surface {
        screen: "identity.screen".into(),
        document: json!({"namespace":"identity","screen":{"type":"screen","controls":[
            {"head":{"type":"image","texture":"","size":[64,64],"offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left",
                "bindings":[{"binding_name":identity::HEAD,"binding_name_override":"#texture"},
                {"binding_name":identity::AVAILABLE,"binding_name_override":"#visible"}]}},
            {"neutral":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[64,64],"offset":[20,20],
                "anchor_from":"top_left","anchor_to":"top_left","radius":0,"color":[0,1,1,1],
                "bindings":[{"binding_name":identity::UNAVAILABLE,"binding_name_override":"#visible"}]}},
            {"name":{"type":"label","text":identity::NAME,"size":[160,20],"offset":[20,90],
                "anchor_from":"top_left","anchor_to":"top_left","bindings":[{"binding_name":identity::NAME}]}}
        ]}}).to_string(),
        // Host values must replace extension attempts to claim another identity.
        bindings: [(identity::NAME.into(), SurfaceValue::Text("Forged".into())),
            (identity::AVAILABLE.into(), SurfaceValue::Bool(true))].into(),
        collections: Default::default(), transitions: Default::default(),    }
}

/// A personal panel with a portrait, native name and original neutral fallback artwork.
fn presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    let mut panel = mod_ui::test_support::sample_panel();
    panel.surface = identity_surface();
    presentation
        .mod_ui_mut()
        .set_mod_panel(mod_ui::PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    presentation
}

/// Paints the host through the same build/atlas path used by the native client.
fn draw(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) -> Vec<u8> {
    rasterize(
        &presentation
            .build(
                &player_state::PlayerState::new(1),
                runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap(),
    )
    .into_raw()
}

/// Checks a whole rendered image for an exact original fixture texel.
fn contains(image: &[u8], color: [u8; 4]) -> bool {
    image.as_chunks::<4>().0.contains(&color)
}

#[test]
fn local_player_identity_skin_changes_replace_rendered_face_and_preserve_hat() {
    let mut presentation = presentation();
    let runtime = UiRuntime::new(1);
    let mut world = profile_world(1, "First", PlayerSkin::Standard(skin(FACE)));
    presentation.capture_local_player_identity(Some(world.local_player_ui()), 1);
    let first = draw(&mut presentation, &runtime);
    assert!(contains(&first, FACE));
    assert!(contains(&first, HAT));
    assert!(!contains(&first, NEUTRAL));
    assert_eq!(presentation.local_player_identity.name(), "First");
    let passes = presentation.mod_ui().mod_panel_state().unwrap().passes;
    assert_eq!(first, draw(&mut presentation, &runtime));
    assert_eq!(
        passes,
        presentation.mod_ui().mod_panel_state().unwrap().passes
    );
    world.sync_local_player_pose(&feed("First", PlayerSkin::Standard(skin(SECOND))));
    presentation.capture_local_player_identity(Some(world.local_player_ui()), 1);
    let second = draw(&mut presentation, &runtime);
    assert!(contains(&second, SECOND));
    assert!(contains(&second, HAT));
    assert!(!contains(&second, FACE));
}

#[test]
fn local_player_identity_missing_and_invalid_skins_render_only_neutral_fallback() {
    let mut presentation = presentation();
    let runtime = UiRuntime::new(1);
    let valid = profile_world(1, "First", PlayerSkin::Standard(skin(FACE)));
    presentation.capture_local_player_identity(Some(valid.local_player_ui()), 1);
    draw(&mut presentation, &runtime);
    for candidate in [
        None,
        Some(profile_world(
            1,
            "Hidden",
            PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        )),
        Some(profile_world(
            1,
            "Malformed",
            PlayerSkin::Standard(StandardSkin {
                rgba8: vec![0; 4].into(),
                ..skin(FACE)
            }),
        )),
    ] {
        presentation.capture_local_player_identity(
            candidate.as_ref().map(|stream| stream.local_player_ui()),
            1,
        );
        let fallback = draw(&mut presentation, &runtime);
        assert!(contains(&fallback, NEUTRAL));
        assert!(!contains(&fallback, FACE));
        assert!(!contains(&fallback, HAT));
        assert_eq!(
            presentation.local_player_identity.name(),
            identity::DEFAULT_NAME
        );
    }
}

#[test]
fn local_player_identity_world_and_session_changes_clear_old_portrait_and_name() {
    let mut presentation = presentation();
    let mut runtime = UiRuntime::new(1);
    let first = profile_world(1, "First", PlayerSkin::Standard(skin(FACE)));
    presentation.capture_local_player_identity(Some(first.local_player_ui()), 1);
    draw(&mut presentation, &runtime);
    runtime.begin_session(2);
    let cleared = draw(&mut presentation, &runtime);
    assert!(contains(&cleared, NEUTRAL));
    assert!(!contains(&cleared, FACE));
    assert_eq!(
        presentation.local_player_identity.name(),
        identity::DEFAULT_NAME
    );
    let second = profile_world(2, "Second", PlayerSkin::Standard(skin(SECOND)));
    presentation.capture_local_player_identity(Some(second.local_player_ui()), 2);
    let replaced = draw(&mut presentation, &runtime);
    assert!(contains(&replaced, SECOND));
    assert!(!contains(&replaced, FACE));
    assert_eq!(presentation.local_player_identity.name(), "Second");
    presentation.capture_local_player_identity(None, 2);
    let left_world = draw(&mut presentation, &runtime);
    assert!(contains(&left_world, NEUTRAL));
    assert!(!contains(&left_world, SECOND));
    assert_eq!(
        presentation.local_player_identity.name(),
        identity::DEFAULT_NAME
    );
}

/// Asserts which name was actually laid out and painted, independent of the skin cache.
fn assert_painted_name(presentation: &UiPresentationRuntime, expected: &str) {
    let expected: [u8; 32] = Sha256::digest(expected.as_bytes()).into();
    let forged: [u8; 32] = Sha256::digest(b"UntrustedIdentity").into();
    let names: Vec<_> = presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(layout.key().content_sha256),
            _ => None,
        })
        .collect();
    assert!(names.contains(&expected), "host name was not painted");
    assert!(!names.contains(&forged), "guest identity was painted");
}

#[test]
fn local_player_identity_never_uses_another_players_roster_profile() {
    let mut presentation = presentation();
    let runtime = UiRuntime::new(1);
    let mut world = profile_world(1, "Local", PlayerSkin::Standard(skin(FACE)));
    world
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::PlayerList(
                protocol::PlayerListUpdateEvent {
                    entries: Arc::from([protocol::PlayerListEntry::Add {
                        uuid: [9; 16],
                        unique_id: 9,
                        username: "Other".into(),
                        verified: true,
                        xuid: "".into(),
                        skin: PlayerSkin::Standard(skin(SECOND)),
                    }]),
                },
            )),
            Some(1),
        )
        .unwrap();
    presentation.capture_local_player_identity(Some(world.local_player_ui()), 1);
    let local = draw(&mut presentation, &runtime);
    assert!(contains(&local, FACE));
    assert!(!contains(&local, SECOND));
    assert_painted_name(&presentation, "Local");
    let portrait = presentation
        .local_player_identity
        .portrait
        .as_ref()
        .unwrap()
        .identity();
    let (_, allocation) = test_alloc::measure(|| {
        presentation.capture_local_player_identity(Some(world.local_player_ui()), 1);
        presentation.publish_local_player_identity(1);
    });
    assert_eq!(allocation.calls, 0);
    assert_eq!(
        presentation
            .local_player_identity
            .portrait
            .as_ref()
            .unwrap()
            .identity(),
        portrait
    );

    world.sync_local_player_pose(&feed(
        "Local",
        PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
    ));
    presentation.capture_local_player_identity(Some(world.local_player_ui()), 1);
    let neutral = draw(&mut presentation, &runtime);
    assert!(contains(&neutral, NEUTRAL));
    assert!(!contains(&neutral, SECOND));
    assert_painted_name(&presentation, identity::DEFAULT_NAME);
}
