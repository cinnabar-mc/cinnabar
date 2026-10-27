//! Retained player preferences and camera-rig configuration.

use bevy::prelude::{EulerRot, Quat, Resource, Vec3};
use semantic_input::PerspectiveMode;
use ui::UserSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSettingsError {
    StaleGeneration { previous: u64, actual: u64 },
    NonFiniteFov,
    FovOutOfRange,
}

/// Monotonic settings snapshots prevent stale or malformed UI updates from changing the camera.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraSettingsAuthority {
    generation: u64,
    horizontal_fov_degrees: f32,
    anti_aliasing_samples: u32,
    motion_blur: ui::MotionBlurQuality,
    smaa_mode: ui::SmaaMode,
    perspective: PerspectiveMode,
    configured_perspective: PerspectiveMode,
    pub(super) freelook: bool,
    presented_view: Option<PresentedView>,
    death_rotation: Option<Quat>,
    feel: CameraFeelSettings,
    game_sensitivity: super::look::GameSensitivity,
    rig: Option<CameraRig>,
    preserve_teleport_rotation: bool,
    view_scale: [f32; 2],
    view_tilt: Option<ViewTilt>,
}

/// A packet mod's detached camera pose, presented instead of the body's camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentedView {
    pub position: Vec3,
    pub rotation: Quat,
    /// Look input turns this view instead of the body.
    pub capture_look: bool,
    /// The walking, jump and sneak bindings stop moving the body.
    pub capture_movement: bool,
}

/// A mod's current-frame tilt of the first-person view, in degrees, applied where the native
/// damage shake is.
///
/// Positive pitch turns the view up. Positive roll turns it about the view direction the way
/// the native shake does for a hit without direction: the view's right edge rises, so the
/// world appears to turn clockwise. The native shake of angle `a` from hurt direction `d` is
/// approximately roll `a cos d` and pitch `-a sin d`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTilt {
    pub roll_degrees: f32,
    pub pitch_degrees: f32,
    /// Removes the native damage shake; death rotation and view bob remain.
    pub suppress_hurt_shake: bool,
}

impl ViewTilt {
    /// The tilt as a rotation of the camera in its own frame: pitch about its right axis,
    /// then roll about its view axis.
    #[must_use]
    pub fn rotation(self) -> Quat {
        Quat::from_euler(
            EulerRot::YXZ,
            0.0,
            self.pitch_degrees.to_radians(),
            self.roll_degrees.to_radians(),
        )
    }
}

/// A local mod's third-person boom: camera-local blocks (x right, y up, z back), roll and FOV change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRig {
    pub offset: Vec3,
    pub roll_radians: f32,
    pub fov_delta_degrees: f32,
}

/// Camera feel toggles and scales mirrored from retained settings, already sanitized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFeelSettings {
    pub fov_effects_scale: f32,
    pub distortion_scale: f32,
    pub view_bobbing: ui::ViewBobbing,
    pub cinematic_camera: bool,
    pub camera_shake: bool,
    pub damage_bob: f32,
    /// Java Edition 1.7 player animations, hand motion and view bob.
    pub java_animations: bool,
    /// Whether server-pack custom held-item models are drawn.
    pub show_custom_item_models: bool,
    /// Whether other players' catalog cosmetics and their effects are drawn.
    pub show_other_player_cosmetics: bool,
    pub gamepad_look_sensitivity: f32,
    pub touch_look_sensitivity: f32,
}

impl CameraFeelSettings {
    /// Sanitizes feel settings as one retained camera snapshot.
    fn from_settings(settings: &UserSettings) -> Self {
        let unit = |value: f32| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        Self {
            fov_effects_scale: unit(settings.video.fov_effects_scale),
            distortion_scale: unit(settings.video.distortion_scale),
            view_bobbing: settings.video.view_bobbing,
            cinematic_camera: settings.video.cinematic_camera,
            camera_shake: settings.video.camera_shake,
            damage_bob: unit(settings.video.damage_bob),
            java_animations: settings.video.java_animations,
            show_custom_item_models: settings.video.show_custom_item_models,
            show_other_player_cosmetics: settings.video.show_other_player_cosmetics,
            gamepad_look_sensitivity: settings.controls.gamepad_look_sensitivity,
            touch_look_sensitivity: settings.controls.touch_look_sensitivity,
        }
    }
}

impl Default for CameraSettingsAuthority {
    fn default() -> Self {
        let settings = UserSettings::default();
        Self {
            generation: 0,
            horizontal_fov_degrees: settings.video.horizontal_fov_degrees,
            anti_aliasing_samples: settings.video.anti_aliasing_samples,
            motion_blur: settings.video.motion_blur,
            smaa_mode: settings.video.smaa_mode,
            perspective: settings.gameplay.default_perspective,
            configured_perspective: settings.gameplay.default_perspective,
            freelook: false,
            presented_view: None,
            death_rotation: None,
            feel: CameraFeelSettings::from_settings(&settings),
            game_sensitivity: Default::default(),
            rig: None,
            preserve_teleport_rotation: false,
            view_scale: [1.0, 1.0],
            view_tilt: None,
        }
    }
}

impl CameraSettingsAuthority {
    /// Presents a mod's detached view and routes look input to it while it captures look;
    /// release returns look to the body. The look pass has already resolved whether a held
    /// user freelook is admitted.
    pub fn apply_presented_view(
        &mut self,
        view: &mut crate::local_player::LocalViewPose,
        presented: Option<PresentedView>,
        user_freelook: bool,
    ) {
        let previous = self.captures_look();
        self.set_presented_view(presented);
        let capture = self.captures_look();
        if capture || previous {
            view.set_freelook(capture || user_freelook && self.freelook);
        }
    }

    /// Atomically accepts newer settings after validating their FOV.
    pub fn replace(
        &mut self,
        generation: u64,
        settings: &UserSettings,
    ) -> Result<(), CameraSettingsError> {
        if generation <= self.generation {
            return Err(CameraSettingsError::StaleGeneration {
                previous: self.generation,
                actual: generation,
            });
        }
        let fov = settings.video.horizontal_fov_degrees;
        if !fov.is_finite() {
            return Err(CameraSettingsError::NonFiniteFov);
        }
        if !(ui::MIN_FOV_DEGREES as f32..=ui::MAX_FOV_DEGREES as f32).contains(&fov) {
            return Err(CameraSettingsError::FovOutOfRange);
        }
        self.generation = generation;
        self.horizontal_fov_degrees = fov;
        self.anti_aliasing_samples = settings.video.anti_aliasing_samples;
        self.motion_blur = settings.video.motion_blur;
        self.smaa_mode = settings.video.smaa_mode;
        if self.configured_perspective != settings.gameplay.default_perspective {
            self.configured_perspective = settings.gameplay.default_perspective;
            self.perspective = self.configured_perspective;
        }
        self.feel = CameraFeelSettings::from_settings(settings);
        self.game_sensitivity
            .set_sensitivity(settings.controls.mouse_sensitivity);
        Ok(())
    }

    #[must_use]
    /// Identifies the latest accepted settings snapshot.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the requested sample count; the renderer clamps it to device support.
    pub const fn anti_aliasing_samples(&self) -> u32 {
        self.anti_aliasing_samples
    }

    /// Returns the optional camera-only motion blur quality.
    pub const fn motion_blur(&self) -> ui::MotionBlurQuality {
        self.motion_blur
    }

    /// Returns the spatial filter preference independently of device sample counts.
    pub const fn smaa_mode(&self) -> ui::SmaaMode {
        self.smaa_mode
    }

    /// The Bedrock FOV setting is vertical for a full-window viewport. The
    /// legacy accessor name is retained with the settings storage field.
    #[must_use]
    pub const fn horizontal_fov_degrees(&self) -> f32 {
        self.horizontal_fov_degrees
    }

    /// A rig or held freelook presents third-person-back so the local body renders and the viewmodel hides.
    /// While the death view is on, first person also presents third-person-back so the body's death
    /// shows; a chosen third-person mode is kept.
    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        if self.presented_view.is_some()
            || self.rig.is_some()
            || self.freelook
            || (self.death_rotation.is_some()
                && matches!(self.perspective, PerspectiveMode::FirstPerson))
        {
            PerspectiveMode::ThirdPersonBack
        } else {
            self.perspective
        }
    }

    /// Whether the local player's death currently overrides first person.
    #[must_use]
    pub const fn death_view(&self) -> bool {
        self.death_rotation.is_some()
    }

    /// The fixed camera heading while the local player is dead: level, facing the way the body
    /// faced when it died. Look input does not turn it.
    #[must_use]
    pub const fn death_rotation(&self) -> Option<Quat> {
        self.death_rotation
    }

    /// Turns the death view on while the local player is dead, snapping the camera to a level
    /// heading from `body_rotation`'s yaw. The stored perspective is untouched, so clearing it
    /// restores the player's own mode.
    pub fn set_death_view(&mut self, body_rotation: Option<Quat>) {
        self.death_rotation = body_rotation.map(|rotation| {
            let (yaw, _, _) = rotation.to_euler(EulerRot::YXZ);
            Quat::from_rotation_y(yaw)
        });
    }

    #[must_use]
    /// Returns the optional local camera rig override.
    pub const fn rig(&self) -> Option<CameraRig> {
        self.rig
    }

    /// Selects a detached view; a non-finite pose or none releases local view ownership.
    pub fn set_presented_view(&mut self, view: Option<PresentedView>) {
        self.presented_view =
            view.filter(|view| view.position.is_finite() && view.rotation.is_finite());
    }

    /// The current mod-presented view, without changing the player's saved perspective.
    pub const fn presented_view(&self) -> Option<PresentedView> {
        self.presented_view
    }

    /// Whether look input turns the presented view instead of the body.
    pub fn captures_look(&self) -> bool {
        self.presented_view.is_some_and(|view| view.capture_look)
    }

    /// Whether the walking, jump and sneak bindings stop moving the body.
    pub fn captures_movement(&self) -> bool {
        self.presented_view
            .is_some_and(|view| view.capture_movement)
    }

    /// A granted extension's current-frame policy for actual teleport yaw/pitch only.
    pub const fn preserves_teleport_rotation(&self) -> bool {
        self.preserve_teleport_rotation
    }

    /// Position reconciliation and teleport acknowledgment remain server-owned.
    pub fn set_preserve_teleport_rotation(&mut self, enabled: bool) {
        self.preserve_teleport_rotation = enabled;
    }

    /// Applies ephemeral view multipliers without changing saved FOV, sensitivity or perspective.
    /// Invalid native values restore the neutral view.
    pub fn set_view_scale(&mut self, fov_scale: f32, look_scale: f32) {
        self.view_scale = if fov_scale.is_finite()
            && look_scale.is_finite()
            && fov_scale > 0.0
            && fov_scale <= 1.0
            && look_scale > 0.0
            && look_scale <= 1.0
        {
            [fov_scale, look_scale]
        } else {
            [1.0, 1.0]
        };
    }

    /// Current-frame multiplier applied after ordinary FOV effects.
    pub const fn fov_scale(&self) -> f32 {
        self.view_scale[0]
    }

    /// Current-frame multiplier applied to calibrated look deltas for every input mode.
    pub const fn look_scale(&self) -> f32 {
        self.view_scale[1]
    }

    /// Applies a mod's current-frame view tilt; non-finite angles restore the native view.
    /// Saved preferences, look and movement are unchanged.
    pub fn set_view_tilt(&mut self, tilt: Option<ViewTilt>) {
        self.view_tilt =
            tilt.filter(|tilt| tilt.roll_degrees.is_finite() && tilt.pitch_degrees.is_finite());
    }

    /// The current mod view tilt, if any.
    pub const fn view_tilt(&self) -> Option<ViewTilt> {
        self.view_tilt
    }

    /// The tilt's camera-frame rotation; identity without one.
    pub fn view_tilt_rotation(&self) -> Quat {
        self.view_tilt.map_or(Quat::IDENTITY, ViewTilt::rotation)
    }

    /// Damage shake strength: the player's damage bob, or none while a tilt suppresses it.
    pub fn hurt_shake_intensity(&self) -> f32 {
        if self.view_tilt.is_some_and(|tilt| tilt.suppress_hurt_shake) {
            0.0
        } else {
            self.feel.damage_bob
        }
    }

    /// Non-finite rigs are dropped; `None` restores the player's own perspective.
    pub fn set_rig(&mut self, rig: Option<CameraRig>) {
        self.rig = rig.filter(|rig| {
            rig.offset.is_finite()
                && rig.roll_radians.is_finite()
                && rig.fov_delta_degrees.is_finite()
        });
    }

    #[must_use]
    /// Returns the sanitized camera feel settings.
    pub const fn feel(&self) -> &CameraFeelSettings {
        &self.feel
    }

    #[must_use]
    /// Vanilla's game sensitivity as left by every accepted sensitivity change.
    pub const fn game_sensitivity(&self) -> f32 {
        self.game_sensitivity.value()
    }

    /// Advances the configured first-person and third-person camera cycle.
    pub fn cycle_perspective(&mut self) {
        self.perspective = next_perspective(self.perspective);
    }

    /// Restores first person and releases freelook.
    pub fn reset_perspective(&mut self) {
        self.perspective = PerspectiveMode::FirstPerson;
        self.freelook = false;
        self.presented_view = None;
        self.death_rotation = None;
        self.preserve_teleport_rotation = false;
        self.view_scale = [1.0, 1.0];
        self.view_tilt = None;
    }
}

#[must_use]
/// Follows the vanilla first/back/front perspective cycle.
pub const fn next_perspective(current: PerspectiveMode) -> PerspectiveMode {
    match current {
        PerspectiveMode::FirstPerson => PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonBack => PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::ThirdPersonFront => PerspectiveMode::FirstPerson,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Camera settings accept both ends of the Video slider's FOV range.
    #[test]
    fn camera_accepts_shared_fov_bounds() {
        for degrees in [ui::MIN_FOV_DEGREES, ui::MAX_FOV_DEGREES] {
            let mut camera = CameraSettingsAuthority::default();
            let mut settings = UserSettings::default();
            settings.video.horizontal_fov_degrees = degrees as f32;
            camera.replace(1, &settings).unwrap();
            assert_eq!(camera.horizontal_fov_degrees(), degrees as f32);
        }
    }

    #[test]
    fn camera_authority_publishes_the_custom_item_model_preference() {
        let mut settings = UserSettings::default();
        settings.video.show_custom_item_models = false;
        let mut camera = CameraSettingsAuthority::default();
        camera.replace(1, &settings).unwrap();
        assert!(!camera.feel().show_custom_item_models);
    }

    #[test]
    fn camera_authority_publishes_the_other_player_cosmetics_preference() {
        let mut settings = UserSettings::default();
        let mut camera = CameraSettingsAuthority::default();
        assert!(camera.feel().show_other_player_cosmetics);
        settings.video.show_other_player_cosmetics = false;
        camera.replace(1, &settings).unwrap();
        assert!(!camera.feel().show_other_player_cosmetics);
        settings.video.show_other_player_cosmetics = true;
        camera.replace(2, &settings).unwrap();
        assert!(camera.feel().show_other_player_cosmetics);
    }
}
