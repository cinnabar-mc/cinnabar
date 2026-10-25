//! Keeps each local world's menu thumbnail current, as vanilla saves a world icon: while the
//! player is in the world with no menu open, a frame is captured now and then, cropped to the
//! view around the crosshair and stored beside the world.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bevy::{
    image::Image,
    prelude::{App, IntoScheduleConfigs, Last, Res, ResMut, Resource, Time, Update},
    time::Real,
};
use client_shell::ui_resources::UiRuntimeResource;
use client_shell::{local_worlds::LocalWorlds, menu::MenuRuntime};
use client_state::{PlayerRuntime, ScreenFocus};
use client_ui::ui_runtime::{
    UiRuntime,
    scene_stack::{Scene, SceneHost},
};
use launcher::local_worlds::world::THUMBNAIL_SIZE;

use super::screenshot::{FrameCapture, FrameCaptureSet};

/// The first capture waits for the terrain around the player to load.
const FIRST_CAPTURE: Duration = Duration::from_secs(10);
/// Later captures follow the player around the world at this pace.
const RECAPTURE: Duration = Duration::from_secs(60);
/// The share of each frame dimension the thumbnail keeps around its centre, leaving out the
/// hotbar, the held item and the status bars at the frame's bottom and sides.
const CROP_SHARE: u32 = 70;
const JPEG_QUALITY: u8 = 80;

/// Encoded JPEG thumbnails by world id, handed from the encoder to the store system.
type EncodedThumbnails = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// Which world is being captured, when its next capture is due, and finished thumbnails
/// waiting to be stored.
#[derive(Resource, Default)]
struct WorldThumbnailCapture {
    world: Option<String>,
    next_at: Option<Duration>,
    encoded: EncodedThumbnails,
}

pub(super) fn configure(app: &mut App) {
    app.init_resource::<WorldThumbnailCapture>()
        .add_systems(Update, store_world_thumbnails)
        .add_systems(Last, capture_world_thumbnail.before(FrameCaptureSet));
}

impl WorldThumbnailCapture {
    /// Keeps a due capture pending until gameplay owns a visible world frame.
    fn due_world(&mut self, now: Duration, playing: Option<&str>, allowed: bool) -> Option<String> {
        if playing != self.world.as_deref() {
            self.next_at = playing.map(|_| now + FIRST_CAPTURE);
            self.world = playing.map(str::to_owned);
        }
        let due = self.next_at?;
        if now < due || !allowed {
            return None;
        }
        self.next_at = Some(now + RECAPTURE);
        self.world.clone()
    }
}

/// Requires the world scene to be visible and own input, including personal UI panels.
fn gameplay_capture_allowed(
    ui: &UiRuntime,
    player: &player_state::PlayerState,
    menu: Option<&MenuRuntime>,
    focus: Option<&ScreenFocus>,
) -> bool {
    if menu.is_some_and(MenuRuntime::is_visible)
        || focus.is_some_and(|focus| focus.menu_visible || focus.ui_focused || focus.absorbs_input)
    {
        return false;
    }
    let host = SceneHost::of(
        menu.map(|menu| {
            menu as &dyn client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene
        }),
        ui,
    );
    let scenes = ui.scenes(player, host);
    scenes.contains(Scene::Gameplay)
        && scenes.receives_input(Scene::Gameplay)
        && scenes.visible(false).contains(&Scene::Gameplay)
}

/// Stores the thumbnails captures have finished.
fn store_world_thumbnails(state: Res<WorldThumbnailCapture>, worlds: Option<ResMut<LocalWorlds>>) {
    let finished =
        std::mem::take(&mut *state.encoded.lock().unwrap_or_else(PoisonError::into_inner));
    if let Some(mut worlds) = worlds {
        for (id, image) in finished {
            worlds.save_thumbnail(&id, image);
        }
    }
}

/// Requests this frame's pixels for the open world's thumbnail once a capture is due.
#[allow(clippy::too_many_arguments)]
fn capture_world_thumbnail(
    time: Res<Time<Real>>,
    menu: Option<Res<MenuRuntime>>,
    ui: Res<UiRuntimeResource>,
    player: Res<PlayerRuntime>,
    focus: Option<Res<ScreenFocus>>,
    mut state: ResMut<WorldThumbnailCapture>,
    mut capture: ResMut<FrameCapture>,
) {
    let now = time.elapsed();
    let playing = menu.as_deref().and_then(MenuRuntime::playing_local_world);
    let allowed = playing.is_some()
        && state.next_at.is_some_and(|due| now >= due)
        && gameplay_capture_allowed(&ui, &player, menu.as_deref(), focus.as_deref());
    let Some(id) = state.due_world(now, playing, allowed) else {
        return;
    };
    let encoded = Arc::clone(&state.encoded);
    capture.request_encoded(move |frame, paper_white_nits| {
        if let Some(image) = encode_thumbnail(frame, paper_white_nits) {
            encoded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((id, image));
        }
    });
}

/// The frame cropped to a 16:9 view around its centre, a little above it, shrunk to
/// [`THUMBNAIL_SIZE`] and encoded as JPEG. An HDR frame is decoded with `paper_white_nits`.
fn encode_thumbnail(frame: Image, paper_white_nits: f32) -> Option<Vec<u8>> {
    let pixels = super::screenshot::sdr::rgb8(frame, paper_white_nits).ok()?;
    let (width, height) = pixels.dimensions();
    let [x, y, crop_width, crop_height] = thumbnail_crop(width, height)?;
    let view = image::imageops::crop_imm(&pixels, x, y, crop_width, crop_height).to_image();
    let thumbnail = image::imageops::thumbnail(&view, THUMBNAIL_SIZE[0], THUMBNAIL_SIZE[1]);
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
        .encode_image(&thumbnail)
        .ok()?;
    Some(encoded)
}

/// Fits the thumbnail's aspect inside the same central share of both frame dimensions.
fn thumbnail_crop(width: u32, height: u32) -> Option<[u32; 4]> {
    let [aspect_width, aspect_height] = THUMBNAIL_SIZE;
    let divisor = gcd(aspect_width, aspect_height);
    let [aspect_width, aspect_height] = [aspect_width / divisor, aspect_height / divisor];
    let units = (u64::from(width) * u64::from(CROP_SHARE) / 100 / u64::from(aspect_width))
        .min(u64::from(height) * u64::from(CROP_SHARE) / 100 / u64::from(aspect_height))
        as u32;
    if units == 0 {
        return None;
    }
    let (crop_width, crop_height) = (units * aspect_width, units * aspect_height);
    Some([
        (width - crop_width) / 2,
        (height - crop_height) / 3,
        crop_width,
        crop_height,
    ])
}

/// Returns the divisor that reduces the thumbnail dimensions to their aspect ratio.
const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[cfg(test)]
mod tests;
