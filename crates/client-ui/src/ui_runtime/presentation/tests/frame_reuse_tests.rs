//! Publication repeats an unchanged frame without building it, and draws exactly what building
//! every frame draws: every HUD input change and every timed change reaches the frame.
//!
//! Two presentations take the same scripted inputs and clock, one repeating unchanged frames and
//! one building every frame; their published frames must match at every step.

use protocol::{
    ActorEffectAction, ActorEffectEvent, ContainerIdentity, InventoryContentEvent, InventoryEvent,
    NetworkItemStack, PlayerGameMode,
};

use super::*;
use crate::ui_runtime::presentation::{
    FrameReuse, ItemIconFrames, PendingUiPublication, ServerUiPack,
};

/// A presentation over fixture artwork, the minimal UI carrier and a small HUD screen drawing
/// the HUD's own renderers and bindings, which needs no local files.
fn fixture_presentation() -> Option<UiPresentationRuntime> {
    fixture_presentation_with(Arc::new(Download::failed()))
}

/// The HUD image the fixture downloads.
const HUD_IMAGE_URL: &str = "https://example.invalid/hud.png";

/// A download whose progress a test sets, shared by every presentation that reads it.
struct Download(std::sync::Mutex<ui_engine::sources::RemoteImage>);

impl Download {
    fn failed() -> Self {
        Self(std::sync::Mutex::new(
            ui_engine::sources::RemoteImage::Failed,
        ))
    }

    fn set(&self, state: ui_engine::sources::RemoteImage) {
        *self.0.lock().unwrap() = state;
    }
}

impl ui_engine::sources::RemoteSource for Download {
    fn state(&self, _url: &str) -> ui_engine::sources::RemoteImage {
        self.0.lock().unwrap().clone()
    }
}

/// [`fixture_presentation`] downloading [`HUD_IMAGE_URL`] through `downloads`.
fn fixture_presentation_with(
    downloads: Arc<dyn ui_engine::sources::RemoteSource>,
) -> Option<UiPresentationRuntime> {
    use json_ui::vanilla::Reference;
    use serde_json::json;
    let mut presentation =
        UiPresentationRuntime::with_hud(fixture_font(), crate::test_support::fixture_hud())
            .unwrap();
    presentation
        .enable_prepared_json_ui_with_sources(
            crate::ui_runtime::presentation::PreparedJsonUi::new(
                crate::test_support::mini_carrier(),
            )
            .unwrap(),
            downloads,
            ui_engine::sources::ImageWork::inline(),
        )
        .unwrap();
    let bottom = |name: &str, renderer: &str, offset: [i32; 2]| {
        json!({name: {"type": "custom", "renderer": renderer, "size": [81, 9],
            "anchor_from": "bottom_middle", "anchor_to": "bottom_middle", "offset": offset}})
    };
    let label = |name: &str, binding: &str, anchor: &str| {
        json!({name: {"type": "label", "text": binding, "localize": false,
            "anchor_from": anchor, "anchor_to": anchor,
            "bindings": [{"binding_name": binding}]}})
    };
    let screen = |reference: Reference, controls: serde_json::Value, extra: serde_json::Value| {
        let (namespace, name) = reference.as_str().split_once('.').unwrap();
        let mut file =
            json!({"namespace": namespace, (name): {"type": "screen", "controls": controls}});
        file.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::to_vec(&file).unwrap()
    };
    let hud = screen(
        Reference::HudHudScreen,
        json!([
            bottom("hearts", "heart_renderer", [-50, -40]),
            bottom("armor", "armor_renderer", [-50, -50]),
            bottom("hunger", "hunger_renderer", [50, -40]),
            bottom("bubbles", "bubbles_renderer", [50, -50]),
            {"hotbar": {"type": "custom", "renderer": "hotbar_slots_renderer", "size": [182, 22],
                "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
                "bindings": [{"binding_name": "#hotbar_visible",
                    "binding_name_override": "#visible"}]}},
            {"slots": {"type": "grid", "collection_name": "hotbar_items",
                "grid_dimensions": [9, 1], "size": [180, 20],
                "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
                "grid_item_template": "hud.fixture_slot"}},
            {"effects": {"type": "custom", "renderer": "java_mob_effects_renderer",
                "size": [120, 30], "anchor_from": "top_right", "anchor_to": "top_right"}},
            label("level", "#level_number", "bottom_middle"),
            {"downloaded": {"type": "image", "texture": HUD_IMAGE_URL, "size": [16, 16],
                "anchor_from": "top_left", "anchor_to": "top_left", "offset": [0, 20]}},
            {"entrance": {"type": "label", "text": "Entrance", "localize": false,
                "anchor_from": "top_left", "anchor_to": "top_left",
                "anims": ["@hud.fixture_entrance"]}},
            label("item_name", "#item_text", "bottom_middle"),
            label("title", "#hud_title_text_string", "center"),
            label("subtitle", "#hud_subtitle_text_string", "bottom_middle"),
            {"chat": {"type": "grid", "collection_name": "chat_text_grid",
                "grid_dimensions": [1, 10], "size": [200, 90],
                "anchor_from": "bottom_left", "anchor_to": "bottom_left",
                "grid_item_template": "hud.chat_row"}},
            {"boss": {"type": "grid", "collection_name": "boss_bars",
                "grid_dimensions": [1, 4], "size": [200, 36],
                "anchor_from": "top_middle", "anchor_to": "top_middle",
                "grid_item_template": "hud.boss_row"}},
            label("sidebar", "#objective_sidebar_name", "right_middle"),
        ]),
        json!({
            "fixture_entrance": {"anim_type": "alpha", "duration": 0.5, "from": 0.0, "to": 1.0},
            "boss_row": {"type": "label", "text": "#bossName", "localize": false,
                "size": [200, 9], "bindings": [{"binding_name": "#bossName",
                    "binding_type": "collection", "binding_collection_name": "boss_bars"}]},
            "chat_row": {"type": "label", "text": "#chat_text", "localize": false,
                "size": [200, 9], "bindings": [{"binding_name": "#chat_text",
                    "binding_type": "collection", "binding_collection_name": "chat_text_grid"}]},
            "fixture_slot": {"type": "panel", "size": [20, 20], "controls": [
                {"art": {"type": "custom", "renderer": "hotbar_renderer", "size": [20, 20]}},
                {"selected": {"type": "label", "text": "^", "localize": false,
                    "anchor_from": "top_middle", "anchor_to": "top_middle",
                    "bindings": [{"binding_name": "#slot_selected",
                        "binding_name_override": "#visible", "binding_type": "collection",
                        "binding_collection_name": "hotbar_items"}]}},
                {"count": {"type": "label", "text": "#inventory_stack_count", "localize": false,
                    "bindings": [{"binding_name": "#inventory_stack_count",
                        "binding_type": "collection", "binding_collection_name": "hotbar_items"}]}},
            ]},
        }),
    );
    let crosshair = screen(
        Reference::HudCrosshairHudCrosshairScreen,
        json!([{"cursor": {"type": "custom", "renderer": "cursor_renderer", "size": [16, 16]}}]),
        json!({}),
    );
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_fixture.json","ui/crosshair_fixture.json"]}"#.to_vec(),
            ),
            ("ui/hud_fixture.json".into(), hud),
            ("ui/crosshair_fixture.json".into(), crosshair),
        ]],
        ..Default::default()
    });
    Some(presentation)
}

/// The scripted session and the two presentations publishing it.
struct Harness {
    player: player_state::PlayerState,
    runtime: UiRuntime,
    repeating: UiPresentationRuntime,
    building: UiPresentationRuntime,
    now: u64,
    size: [u32; 2],
    perspective: semantic_input::PerspectiveMode,
    /// How the repeating presentation repeats frames.
    reuse: FrameReuse,
    /// Whether the script changed the repeating presentation's inputs since the last frame, as
    /// the changing APIs reported it.
    changed: bool,
    /// The frame the repeating presentation last published.
    published: Option<render_model::UiRenderInput>,
    /// Frames the repeating presentation did not build.
    repeats: usize,
}

impl Harness {
    /// Two presentations from `presentation`, one repeating frames as `reuse` does and one
    /// building every frame.
    fn new(
        presentation: impl Fn() -> Option<UiPresentationRuntime>,
        reuse: FrameReuse,
    ) -> Option<Self> {
        let [mut repeating, mut building] = [presentation()?, presentation()?];
        repeating.set_frame_reuse(reuse);
        building.set_frame_reuse(FrameReuse::Off);
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
        runtime
            .publish_local_runtime_id(&mut player, 1, 42)
            .unwrap();
        Some(Self {
            player,
            runtime,
            repeating,
            building,
            now: 1_000,
            size: [1280, 720],
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            reuse,
            changed: true,
            published: None,
            repeats: 0,
        })
    }

    /// Changes the session through `change`, as a system writing the runtime and player would:
    /// the frame's inputs count as changed only when `change` reports it, as the production
    /// writer's own report or, for a writer that marks its resource changed unconditionally,
    /// `true`.
    fn change(
        &mut self,
        change: impl FnOnce(&mut player_state::PlayerState, &mut UiRuntime) -> bool,
    ) {
        self.changed |= change(&mut self.player, &mut self.runtime);
    }

    /// Changes both presentations' inputs, as a system writing the presentation would; the
    /// repeating presentation's report decides whether its inputs changed.
    fn present(&mut self, change: impl Fn(&mut UiPresentationRuntime) -> bool) {
        self.changed |= change(&mut self.repeating);
        change(&mut self.building);
    }

    /// Captures and publishes one frame on both presentations and checks they draw alike;
    /// whether the repeating one built.
    fn frame(&mut self) -> bool {
        // Background artwork packing and image decodes land at the same frame on both
        // presentations, so neither draws work the other has not received.
        for presentation in [&mut self.repeating, &mut self.building] {
            if !presentation.menu_artwork_loader.settled() {
                presentation.menu_artwork_loader.wait();
            }
            if let Some(engine) = presentation.engine.as_deref() {
                test_time::eventually("background image work", || !engine.image_work_in_flight());
            }
        }
        // Publication preparation expires finished HUD output first, as the app's does.
        let changed = std::mem::take(&mut self.changed) | self.runtime.expire_hud(self.now);
        let (repeated, mut fresh) = self.round(changed);
        let built = repeated.is_some();
        match repeated {
            Some(input) => self.published = Some(input),
            None => self.repeats += 1,
        }
        // The closed inventory prepares on a worker that each build services, and frames keep
        // building while it does: both presentations finish it, unchanged frames and all,
        // before they compare.
        test_time::eventually("inventory preparation under repeated frames", || {
            if !self.preparing() {
                return true;
            }
            let (repeated, built) = self.round(false);
            if let Some(repeated) = repeated {
                self.published = Some(repeated);
            }
            fresh = built;
            !self.preparing()
        });
        let mut repeated = self.published.clone().expect("a first frame built");
        (repeated.revision, fresh.revision) = (0, 0);
        assert!(
            draws_alike(&repeated, &fresh),
            "the repeated frame at {} ms differs from a fresh build: {}",
            self.now,
            difference(&repeated, &fresh)
        );
        built
    }

    /// Whether either presentation is still preparing the closed inventory.
    fn preparing(&self) -> bool {
        self.repeating.engine_inventory_preparation_pending()
            || self.building.engine_inventory_preparation_pending()
    }

    /// Captures and publishes on both presentations; the repeating one's frame unless it
    /// repeated, and the building one's. `changed` adds the script's changes to what the
    /// captures report: the runtime both write, and the repeating presentation's own inputs.
    fn round(
        &mut self,
        mut changed: bool,
    ) -> (
        Option<render_model::UiRenderInput>,
        render_model::UiRenderInput,
    ) {
        let mut repeating = true;
        let captures = [&mut self.repeating, &mut self.building].map(|presentation| {
            let capture = crate::ui_runtime::presentation::capture_hud_frame(
                &self.player,
                &mut self.runtime,
                presentation,
                None,
                self.perspective == semantic_input::PerspectiveMode::FirstPerson,
                self.now,
                ItemIconFrames::default(),
            );
            changed |= capture.runtime_changed | (repeating && capture.presentation_changed);
            repeating = false;
            capture.icons
        });
        let [repeating_icons, building_icons] = captures;
        let mut publish = |presentation: &mut UiPresentationRuntime, icons| {
            let prepared = PendingUiPublication {
                inventory: self.runtime.capture_presentation_inventory(&self.player),
                preview: PreviewCapture {
                    skin: None,
                    ready: false,
                    pose: Default::default(),
                    shown: false,
                    hands: false,
                },
                item_icons: icons,
                now_millis: self.now,
                physical_size: self.size,
                dpi_scale: DpiScale::new(1.0).unwrap(),
            };
            crate::ui_runtime::presentation::render_prepared_ui(
                &self.player,
                &mut self.runtime,
                presentation,
                prepared,
                !changed,
            )
            .unwrap()
        };
        let repeated = publish(&mut self.repeating, repeating_icons);
        let fresh = publish(&mut self.building, building_icons)
            .expect("the building presentation always builds");
        (repeated, fresh)
    }

    /// Publishes frames every `step` milliseconds for `span` milliseconds.
    fn run(&mut self, span: u64, step: u64) {
        for _ in 0..span / step {
            self.now += step;
            self.frame();
        }
    }

    /// The repeating presentation's revision.
    fn revision(&self) -> u64 {
        self.published.as_ref().map_or(0, |input| input.revision)
    }

    /// Publishes a frame after a change and, when the HUD draws it (`strict`), checks it drew
    /// something new.
    fn assert_changes(&mut self, what: &str, strict: bool) {
        let before = self.revision();
        self.now += 16;
        let built = self.frame();
        if strict {
            assert!(built, "{what}: the frame after the change must build");
            assert_ne!(
                self.revision(),
                before,
                "{what}: the published frame must change ({} vertices)",
                self.published
                    .as_ref()
                    .map_or(0, |input| input.vertices.len())
            );
        }
    }
}

/// Whether two frames draw alike: equal, or with equal geometry over texture pages that differ
/// only where nothing samples them. Independent presentations may pack art pages' unused space
/// differently.
fn draws_alike(a: &render_model::UiRenderInput, b: &render_model::UiRenderInput) -> bool {
    use crate::ui_runtime::presentation::forms::snapshot::rasterize;
    a == b
        || (a.viewport_size == b.viewport_size
            && a.safe_area == b.safe_area
            && a.vertices == b.vertices
            && a.indices == b.indices
            && a.batches == b.batches
            && a.models == b.models
            && a.textures.plan() == b.textures.plan()
            && rasterize(a) == rasterize(b))
}

/// The parts of two frames that differ, for failure messages.
fn difference(a: &render_model::UiRenderInput, b: &render_model::UiRenderInput) -> String {
    let mut parts = Vec::new();
    if a.viewport_size != b.viewport_size || a.safe_area != b.safe_area {
        parts.push("viewport".to_owned());
    }
    if a.vertices != b.vertices {
        let first = a
            .vertices
            .iter()
            .zip(b.vertices.iter())
            .position(|(a, b)| a != b);
        parts.push(format!(
            "vertices ({} vs {}, first differing {first:?})",
            a.vertices.len(),
            b.vertices.len()
        ));
    }
    if a.indices != b.indices {
        parts.push(format!(
            "indices ({} vs {})",
            a.indices.len(),
            b.indices.len()
        ));
    }
    if a.batches != b.batches {
        let first = a
            .batches
            .iter()
            .zip(b.batches.iter())
            .position(|(a, b)| a != b);
        parts.push(format!("batches (first differing {first:?})"));
    }
    if a.textures != b.textures {
        let pages = a.textures.pages().iter().zip(b.textures.pages().iter());
        let differing = pages
            .enumerate()
            .filter(|(_, (a, b))| a.pixels() != b.pixels())
            .map(|(index, (page, _))| (index, page.resident_dimensions()))
            .collect::<Vec<_>>();
        parts.push(format!(
            "textures ({} vs {} pages, pixels differ on {differing:?}, same plan {})",
            a.textures.pages().len(),
            b.textures.pages().len(),
            a.textures.plan() == b.textures.plan()
        ));
    }
    if a.models != b.models {
        parts.push("models".to_owned());
    }
    parts.join(", ")
}

impl Harness {
    /// Publishes a frame at the same time after a change its API reported as none, and checks it
    /// repeated without building.
    fn assert_repeats(&mut self, what: &str) {
        let built = self.frame();
        if self.reuse == FrameReuse::Skip {
            assert!(!built, "{what}: an unchanged frame must repeat");
        }
    }
}

fn item(network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: network_id,
        count,
        nbt_digest: <sha2::Sha256 as sha2::Digest>::digest([]).into(),
        block_runtime_id: 0,
        extra_data: std::sync::Arc::from([]),
    }
}

fn effect(effect_id: i32, duration_ticks: i32) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action: ActorEffectAction::Add,
        effect_id,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks,
        tick: 0,
    }
}

fn stat(current: u16, maximum: u16) -> Option<BoundedStat> {
    BoundedStat::new(current, maximum)
}

/// The full HUD over the installed UI carrier: every change reaches the frame, and repeated
/// frames match fresh builds through each timed change.
#[test]
fn hud_changes_and_fades_draw_as_fresh_builds_while_unchanged_frames_repeat() {
    let Some(mut harness) =
        Harness::new(crate::test_support::engine_presentation, FrameReuse::Skip)
    else {
        eprintln!(
            "skipping hud_changes_and_fades_draw_as_fresh_builds_while_unchanged_frames_repeat: \
             fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    script(&mut harness, true);
}

/// Without local files the HUD draws little, but repeated frames still match fresh builds and
/// idle frames repeat.
#[test]
fn repeated_frames_match_fresh_builds_without_local_carriers() {
    let mut harness = Harness::new(fixture_presentation, FrameReuse::Skip).unwrap();
    script(&mut harness, false);
}

/// A HUD image whose download finishes while frames repeat draws on the next frame.
#[test]
fn a_download_finishing_while_frames_repeat_draws_next_frame() {
    let download = Arc::new(Download(std::sync::Mutex::new(
        ui_engine::sources::RemoteImage::Loading,
    )));
    let shared = Arc::clone(&download);
    let mut harness = Harness::new(
        move || fixture_presentation_with(shared.clone()),
        FrameReuse::Skip,
    )
    .unwrap();
    assert!(harness.frame());
    harness.run(1_000, 16);
    let repeats = harness.repeats;
    harness.run(200, 16);
    assert!(
        harness.repeats > repeats + 10,
        "idle frames repeat while it loads"
    );
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 30, 30, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    download.set(ui_engine::sources::RemoteImage::Ready(bytes.into()));
    harness.assert_changes("downloaded HUD image", true);
}

/// A HUD image hidden before its download finishes makes one frame build when it settles; with
/// nothing drawing it, idle frames repeat again.
#[test]
fn a_download_settling_after_its_image_hides_lets_idle_frames_repeat() {
    let download = Arc::new(Download(std::sync::Mutex::new(
        ui_engine::sources::RemoteImage::Loading,
    )));
    let shared = Arc::clone(&download);
    let mut harness = Harness::new(
        move || fixture_presentation_with(shared.clone()),
        FrameReuse::Skip,
    )
    .unwrap();
    assert!(harness.frame());
    harness.run(1_000, 16);
    harness.present(|presentation| {
        let mut settings = launcher::menu::settings_options::SettingsOptions::default();
        settings.set(
            launcher::menu::settings_options::SettingId::HideHud.index(),
            1,
        );
        presentation.set_chat_settings_snapshot((std::sync::Arc::new(settings), None))
    });
    harness.assert_changes("hidden HUD", true);
    download.set(ui_engine::sources::RemoteImage::Failed);
    harness.run(200, 16);
    let repeats = harness.repeats;
    harness.run(200, 16);
    assert!(
        harness.repeats > repeats + 10,
        "idle frames repeat once the hidden image's download settled"
    );
}

/// A capture of unchanged inputs reports no change, whichever first-person state the HUD draws
/// for, including a third-person or hidden hand.
#[test]
fn captures_of_unchanged_inputs_report_no_change() {
    let mut presentation = fixture_presentation().unwrap();
    let player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for first_person in [true, false, true] {
        let mut capture = || {
            crate::ui_runtime::presentation::capture_hud_frame(
                &player,
                &mut runtime,
                &mut presentation,
                None,
                first_person,
                1_000,
                ItemIconFrames::default(),
            )
        };
        capture();
        let again = capture();
        assert!(
            !again.runtime_changed && !again.presentation_changed,
            "an unchanged capture with first_person={first_person} reports a change"
        );
    }
}

/// Auditing repeats replays them as skipping does and periodically builds one to compare: the
/// script's frames still match fresh builds.
// The audit compares only in debug builds.
#[cfg(debug_assertions)]
#[test]
fn audited_repeats_match_fresh_builds() {
    let mut harness = Harness::new(fixture_presentation, FrameReuse::Audit).unwrap();
    script(&mut harness, false);
}

/// Scripted HUD changes, timed fades and idle stretches; `strict` checks each change draws.
fn script(harness: &mut Harness, carriers: bool) {
    assert!(harness.frame(), "the first frame builds");
    // Idle frames repeat without building once entrance animations settle.
    harness.run(1_000, 16);
    assert!(
        harness.repeats > 25,
        "idle frames repeat: {}",
        harness.repeats
    );

    // A sidebar; first, as its events take the earliest sequence numbers.
    harness.change(|player, runtime| {
        super::retained_hud_tests::install_mixed_scoreboard_slot(
            player,
            runtime,
            "sidebar",
            &[(
                20,
                ProtocolScoreIdentity::FakePlayer(std::sync::Arc::from("Row")),
                1,
            )],
        );
        true
    });
    harness.assert_changes("sidebar", true);

    // The hotbar shows with a selected slot, then its contents and selection change.
    harness.change(|player, _| {
        player.inventory.set_local_selected_slot(0);
        true
    });
    harness.assert_changes("hotbar shown", true);
    harness.change(|player, runtime| {
        let mut slots = vec![NetworkItemStack::empty(); 36];
        (slots[0], slots[1]) = (item(1, 3), item(2, 64));
        runtime
            .inventory_ledger_mut(player)
            .apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
        true
    });
    harness.assert_changes("hotbar contents", true);
    harness.change(|player, _| {
        player.inventory.set_local_selected_slot(1);
        true
    });
    harness.assert_changes("selected slot", true);
    // The selected item's name shows, then fades with no further input.
    harness.run(4_000, 50);

    // Survival rows: health, hunger, armor and air, then experience.
    harness.change(|player, runtime| {
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        runtime
            .session
            .hud
            .set_stats(stat(20, 20), stat(20, 20), stat(4, 20), stat(300, 300));
        true
    });
    harness.assert_changes("survival rows", true);
    harness.change(|_, runtime| {
        let hud = &mut runtime.session.hud;
        hud.set_stats(stat(7, 20), hud.hunger(), hud.armor(), hud.air());
        true
    });
    harness.assert_changes("health", true);
    harness.change(|_, runtime| {
        let hud = &mut runtime.session.hud;
        hud.set_stats(hud.health(), stat(3, 20), hud.armor(), hud.air());
        true
    });
    harness.assert_changes("hunger", true);
    harness.change(|_, runtime| {
        let hud = &mut runtime.session.hud;
        hud.set_stats(hud.health(), hud.hunger(), hud.armor(), stat(120, 300));
        true
    });
    harness.assert_changes("air", true);
    harness.change(|_, runtime| {
        runtime.session.hud.set_experience(17, 0.25);
        true
    });
    harness.assert_changes("experience", true);
    harness.run(1_000, 16);

    // A timed effect shows, blinks out and expires on the session clock.
    let at = harness.now + 16;
    harness.change(|_, runtime| {
        runtime.observe_presentation_tick(1, at);
        runtime
            .apply_local_effect(1, 3, effect(1, 120), at)
            .unwrap();
        true
    });
    harness.assert_changes("effect", true);
    harness.run(8_000, 50);

    // Chat lines arrive and fade.
    let at = harness.now + 16;
    harness.change(|player, runtime| {
        runtime
            .apply(
                player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 4,
                    local_millis: at,
                    server_tick: None,
                    event: chat_event("hello there"),
                },
            )
            .unwrap();
        true
    });
    harness.assert_changes("chat line", true);
    harness.run(12_000, 100);

    // A title and subtitle fade in, hold and fade out; then the action bar shows and fades.
    let at = harness.now;
    harness.change(|_, runtime| {
        let hud = &mut runtime.session.hud;
        hud.set_durations(ui::TitleDurations::from_wire(10, 20, 10).unwrap());
        hud.set_subtitle(std::sync::Arc::from("Fight"), 4, at);
        hud.set_title(std::sync::Arc::from("Round 2"), 5, at);
        true
    });
    harness.assert_changes("title and subtitle", true);
    harness.run(3_000, 25);
    let at = harness.now + 16;
    harness.change(|_, runtime| {
        runtime
            .session
            .hud
            .set_actionbar(std::sync::Arc::from("Action"), 6, at);
        true
    });
    harness.assert_changes("action bar", carriers);
    harness.run(4_000, 50);

    // A boss bar.
    let at = harness.now + 16;
    harness.change(|player, runtime| {
        runtime
            .apply(
                player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 5,
                    local_millis: at,
                    server_tick: None,
                    event: boss_event(
                        ProtocolBossAction::Show,
                        9,
                        "Boss",
                        0.5,
                        ProtocolBossColor::Red,
                        ProtocolBossOverlay::Progress,
                    ),
                },
            )
            .unwrap();
        true
    });
    harness.assert_changes("boss bar", true);
    harness.run(500, 16);

    // A toast slides in, waits and slides out.
    let at = harness.now;
    harness.change(|_, runtime| {
        runtime.push_client_toast(
            std::sync::Arc::from("Toast"),
            std::sync::Arc::from("Body"),
            at,
        );
        true
    });
    harness.assert_changes("toast", carriers);
    harness.run(7_000, 50);

    // The crosshair follows the perspective; settings, the GUI scale and the window resize.
    harness.perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
    harness.assert_changes("third-person crosshair", carriers);
    harness.perspective = semantic_input::PerspectiveMode::FirstPerson;
    harness.assert_changes("first-person crosshair", carriers);
    // A mod draws on the HUD and stops.
    harness.present(|presentation| {
        presentation
            .mod_ui_mut()
            .set_mod_label(Some("Mod HUD"))
            .unwrap();
        true
    });
    harness.assert_changes("mod HUD drawn", true);
    harness.present(|presentation| {
        presentation.mod_ui_mut().set_mod_label(None).unwrap();
        true
    });
    harness.assert_changes("mod HUD removed", true);
    harness.present(|presentation| {
        presentation.set_gui_scale_preference(Some(1));
        true
    });
    harness.assert_changes("GUI scale", true);
    // The HUD hides right after a repeated frame, then shows again: its controls come back new
    // and play their entrance animations, as when every frame builds.
    for _ in 0..100 {
        harness.now += 5;
        if !harness.frame() {
            break;
        }
    }
    harness.present(|presentation| {
        let mut settings = launcher::menu::settings_options::SettingsOptions::default();
        settings.set(
            launcher::menu::settings_options::SettingId::HideHud.index(),
            1,
        );
        presentation.set_chat_settings_snapshot((std::sync::Arc::new(settings), None))
    });
    harness.assert_changes("hidden HUD", true);
    harness.present(|presentation| {
        presentation.set_chat_settings_snapshot((std::sync::Arc::default(), None))
    });
    harness.assert_changes("shown HUD", true);
    harness.run(700, 16);
    harness.size = [960, 540];
    harness.assert_changes("window resize", true);

    // A screen opens over the HUD and closes; the pack cover comes and goes.
    harness.change(|player, runtime| {
        runtime.open_chat(player);
        true
    });
    harness.assert_changes("chat screen open", true);
    harness.run(300, 16);
    harness.change(|_, runtime| {
        runtime.close_chat();
        true
    });
    harness.assert_changes("chat screen closed", true);
    harness.change(|_, runtime| runtime.set_resource_pack_loading(true));
    harness.assert_changes("pack cover", true);
    harness.change(|_, runtime| runtime.set_resource_pack_loading(false));
    harness.assert_changes("pack cover gone", true);
    // The uncovered HUD's controls play their entrance again.
    harness.run(700, 16);

    // Low hunger shakes every game tick, and the frame builds each tick; restored, it holds.
    harness.change(|_, runtime| {
        let hud = &mut runtime.session.hud;
        hud.set_stats(hud.health(), stat(20, 20), hud.armor(), hud.air());
        true
    });
    harness.assert_changes("hunger restored", true);

    // Idle again: survival rows read the game tick, so frames repeat between ticks.
    let repeats = harness.repeats;
    harness.run(1_000, 5);
    assert!(
        harness.repeats >= repeats + 150,
        "idle frames repeat between game ticks: {}",
        harness.repeats - repeats
    );
    // Writers that change nothing report so, and the frame repeats.
    harness.change(|_, runtime| runtime.set_resource_pack_loading(false));
    harness.assert_repeats("pack cover already gone");
    harness.present(|presentation| {
        presentation.set_chat_settings_snapshot((std::sync::Arc::default(), None))
    });
    harness.assert_repeats("settings unchanged");

    // A resource pack swap restyles the HUD, which then holds.
    harness.present(|presentation| {
        presentation.set_server_ui_pack(&ServerUiPack {
            ui_layers: vec![vec![
                (
                    "ui/_ui_defs.json".into(),
                    br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
                ),
                (
                    "ui/hud_screen.json".into(),
                    br#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[
                        {"label":{"type":"label","text":"Swapped pack","localize":false}}]}}"#
                        .to_vec(),
                ),
            ]],
            ..Default::default()
        });
        true
    });
    harness.assert_changes("resource pack swap", true);
    let repeats = harness.repeats;
    harness.run(500, 5);
    assert!(
        harness.repeats >= repeats + 75,
        "the swapped HUD repeats between game ticks"
    );
}
