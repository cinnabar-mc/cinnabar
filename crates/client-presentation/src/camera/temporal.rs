//! Requests temporal upscaling for the world camera: sub-pixel jitter, texture mip bias and the
//! history reset epoch the renderer feeds to the upscaler.

use bevy::{
    anti_alias::smaa::Smaa,
    prelude::*,
    render::{
        camera::{MipBias, TemporalJitter},
        render_resource::TextureUsages,
    },
};
use render_scene::{
    shaderpack::ShaderPackRendering,
    temporal::{TemporalUpscaler, TemporalUpscalerSupport, TemporalUpscaling},
};

use super::{CameraSettingsAuthority, FlyCamera, antialiasing, cuts::CameraCuts};

/// Jitter phases at native resolution; reduced resolutions scale this by the pixel-area ratio.
const NATIVE_JITTER_PHASES: u32 = 8;

/// The temporal upscaler a player preference selects, if any.
const fn temporal_upscaler(upscaler: ui::Upscaler) -> Option<TemporalUpscaler> {
    match upscaler {
        ui::Upscaler::Fsr1 => None,
        ui::Upscaler::Dlss => Some(TemporalUpscaler::Dlss),
    }
}

/// The world resolution percentage actually rendered; out-of-range values render natively.
const fn effective_render_scale_percent(percent: u8) -> u8 {
    if percent >= ui::MIN_RENDER_SCALE_PERCENT && percent <= ui::MAX_RENDER_SCALE_PERCENT {
        percent
    } else {
        ui::MAX_RENDER_SCALE_PERCENT
    }
}

/// Jitter sequence length for a render scale: `ceil(8 * (output / render)^2)`, never below 8,
/// so every output pixel receives several samples per cycle.
#[must_use]
pub const fn jitter_phase_count(render_scale_percent: u8) -> u32 {
    let percent = effective_render_scale_percent(render_scale_percent) as u32;
    let native = ui::MAX_RENDER_SCALE_PERCENT as u32;
    let area = percent * percent;
    let phases = (NATIVE_JITTER_PHASES * native * native).div_ceil(area);
    if phases < NATIVE_JITTER_PHASES {
        NATIVE_JITTER_PHASES
    } else {
        phases
    }
}

/// Radical inverse of `index` in `base`, the `index`th Halton value in `[0, 1)`.
fn halton(mut index: u32, base: u32) -> f32 {
    let mut scale = 1.0;
    let mut value = 0.0;
    while index > 0 {
        scale /= base as f32;
        value += scale * (index % base) as f32;
        index /= base;
    }
    value
}

/// The camera's sub-pixel offset for `frame`, in render pixels within `[-0.5, 0.5)`: the
/// Halton(2, 3) sequence, skipping its zero first point and repeating every `phases` frames.
#[must_use]
pub fn jitter_offset(frame: u64, phases: u32) -> Vec2 {
    let index = (frame % u64::from(phases.max(1))) as u32 + 1;
    Vec2::new(halton(index, 2), halton(index, 3)) - 0.5
}

/// Sharpens texture sampling by the resolution ratio so upscaled textures keep output detail.
#[must_use]
pub fn mip_bias(render_scale_percent: u8) -> f32 {
    let scale = f32::from(effective_render_scale_percent(render_scale_percent))
        / f32::from(ui::MAX_RENDER_SCALE_PERCENT);
    scale.log2() - 1.0
}

/// World cameras and the components temporal upscaling owns on them.
type TemporalCamera<'a> = (
    Entity,
    &'a mut Camera3d,
    Option<&'a mut TemporalUpscaling>,
    Option<&'a mut TemporalJitter>,
    Option<&'a mut MipBias>,
    Option<&'a mut Msaa>,
    Has<Smaa>,
    Has<ShaderPackRendering>,
);

/// Requests the preferred temporal upscaler on the world camera while the device supports it
/// and no shader pack draws the world; otherwise removes the request, jitter and mip bias.
/// Runs after [`super::cuts::track_camera_cuts`] so a cut resets history on its own frame.
pub fn apply_temporal_upscaling(
    mut commands: Commands,
    settings: Res<CameraSettingsAuthority>,
    support: Option<Res<TemporalUpscalerSupport>>,
    cuts: Res<CameraCuts>,
    time: Option<Res<Time<Real>>>,
    mut frame: Local<u64>,
    mut cameras: Query<TemporalCamera, With<FlyCamera>>,
) {
    let upscaler = temporal_upscaler(settings.upscaler()).filter(|upscaler| {
        support
            .as_ref()
            .is_some_and(|support| support.supports(*upscaler))
    });
    let percent = settings.render_scale_percent();
    let desired_jitter = jitter_offset(*frame, jitter_phase_count(percent));
    let desired_bias = mip_bias(percent);
    let delta_seconds = time.as_ref().map_or(0.0, |time| time.delta_secs());
    let mut jittered = false;
    for (entity, mut camera, request, jitter, bias, msaa, smaa, shader_pack) in &mut cameras {
        let Some(upscaler) = upscaler.filter(|_| !shader_pack) else {
            if request.is_some() || jitter.is_some() || bias.is_some() {
                commands
                    .entity(entity)
                    .remove::<(TemporalUpscaling, TemporalJitter, MipBias)>();
            }
            continue;
        };
        jittered = true;
        let usage = TextureUsages::from(camera.depth_texture_usages);
        if !usage.contains(TextureUsages::TEXTURE_BINDING) {
            camera.depth_texture_usages = (usage | TextureUsages::TEXTURE_BINDING).into();
        }
        let desired = TemporalUpscaling {
            upscaler,
            reset_epoch: cuts.epoch(),
            delta_seconds,
        };
        match request {
            Some(mut request) if *request != desired => *request = desired,
            Some(_) => {}
            None => {
                commands.entity(entity).insert(desired);
                // Single sampling must hold on the first extracted frame, before the
                // anti-aliasing pass next observes the request.
                antialiasing::disable_for_temporal_upscaling(&mut commands, entity, msaa, smaa);
            }
        }
        match jitter {
            Some(mut jitter) if jitter.offset != desired_jitter => jitter.offset = desired_jitter,
            Some(_) => {}
            None => {
                commands.entity(entity).insert(TemporalJitter {
                    offset: desired_jitter,
                });
            }
        }
        match bias {
            Some(mut bias) if bias.0 != desired_bias => bias.0 = desired_bias,
            Some(_) => {}
            None => {
                commands.entity(entity).insert(MipBias(desired_bias));
            }
        }
    }
    if jittered {
        *frame = frame.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        camera::{ServerCameraView, cuts::track_camera_cuts},
        local_player::LocalViewPose,
    };
    use std::collections::HashSet;

    #[test]
    fn phase_count_scales_with_pixel_area_and_never_drops_below_eight() {
        assert_eq!(jitter_phase_count(100), 8);
        assert_eq!(jitter_phase_count(50), 32);
        // 8 * (100 / 67)^2 = 17.82.
        assert_eq!(jitter_phase_count(67), 18);
        assert_eq!(jitter_phase_count(ui::MIN_RENDER_SCALE_PERCENT), 800);
        for invalid in [0, ui::MIN_RENDER_SCALE_PERCENT - 1, 101, u8::MAX] {
            assert_eq!(jitter_phase_count(invalid), 8);
        }
        for percent in ui::MIN_RENDER_SCALE_PERCENT..=ui::MAX_RENDER_SCALE_PERCENT {
            let exact = 8.0 * (100.0 / f64::from(percent)).powi(2);
            let phases = jitter_phase_count(percent);
            assert!(f64::from(phases) >= exact - 1e-9 && f64::from(phases) < exact + 1.0);
        }
    }

    #[test]
    fn jitter_follows_halton_2_3_centered_on_the_pixel_and_repeats_each_cycle() {
        let expected = [
            [0.5, 1.0 / 3.0],
            [0.25, 2.0 / 3.0],
            [0.75, 1.0 / 9.0],
            [0.125, 4.0 / 9.0],
        ];
        for (frame, [x, y]) in expected.into_iter().enumerate() {
            let offset = jitter_offset(frame as u64, 8);
            assert!(
                offset.abs_diff_eq(Vec2::new(x - 0.5, y - 0.5), 1e-6),
                "{frame}"
            );
        }
        for phases in [8, 32, 800] {
            let cycle: Vec<_> = (0..u64::from(phases))
                .map(|frame| jitter_offset(frame, phases))
                .collect();
            for offset in &cycle {
                assert!((-0.5..0.5).contains(&offset.x) && (-0.5..0.5).contains(&offset.y));
            }
            let distinct: HashSet<_> = cycle
                .iter()
                .map(|offset| (offset.x.to_bits(), offset.y.to_bits()))
                .collect();
            assert_eq!(distinct.len(), phases as usize);
            assert_eq!(jitter_offset(u64::from(phases), phases), cycle[0]);
            assert_eq!(jitter_offset(u64::from(phases) * 3 + 5, phases), cycle[5]);
        }
    }

    #[test]
    fn mip_bias_follows_the_resolution_ratio() {
        assert_eq!(mip_bias(100), -1.0);
        assert_eq!(mip_bias(50), -2.0);
        assert_eq!(mip_bias(25), -3.0);
        assert_eq!(mip_bias(0), -1.0);
    }

    fn app(support: Option<TemporalUpscalerSupport>) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<CameraSettingsAuthority>()
            .init_resource::<LocalViewPose>()
            .init_resource::<ServerCameraView>()
            .init_resource::<CameraCuts>()
            .init_resource::<Time<Real>>()
            .add_systems(
                Update,
                (track_camera_cuts, apply_temporal_upscaling).chain(),
            );
        if let Some(support) = support {
            app.insert_resource(support);
        }
        let camera = app
            .world_mut()
            .spawn((
                FlyCamera::default(),
                Camera3d {
                    depth_texture_usages: TextureUsages::RENDER_ATTACHMENT.into(),
                    ..default()
                },
                Msaa::Sample4,
                Smaa::default(),
            ))
            .id();
        (app, camera)
    }

    const DLSS: TemporalUpscalerSupport = TemporalUpscalerSupport { dlss: true };

    fn select(app: &mut App, upscaler: ui::Upscaler, render_scale_percent: u8) {
        let mut settings = ui::UserSettings::default();
        settings.video.upscaler = upscaler;
        settings.video.render_scale_percent = render_scale_percent;
        let mut authority = app.world_mut().resource_mut::<CameraSettingsAuthority>();
        let generation = authority.generation() + 1;
        authority.replace(generation, &settings).unwrap();
    }

    fn requested(app: &App, camera: Entity) -> bool {
        let entity = app.world().entity(camera);
        let parts = [
            entity.contains::<TemporalUpscaling>(),
            entity.contains::<TemporalJitter>(),
            entity.contains::<MipBias>(),
        ];
        assert!(parts.iter().all(|part| *part == parts[0]), "{parts:?}");
        parts[0]
    }

    #[test]
    fn dlss_requests_jitter_bias_and_single_sampling_only_when_selected_and_supported() {
        for support in [None, Some(TemporalUpscalerSupport::default())] {
            let (mut app, camera) = app(support);
            select(&mut app, ui::Upscaler::Dlss, 50);
            app.update();
            assert!(!requested(&app, camera), "{support:?}");
        }
        let (mut app, camera) = app(Some(DLSS));
        app.update();
        assert!(!requested(&app, camera));
        select(&mut app, ui::Upscaler::Dlss, 50);
        app.update();
        assert!(requested(&app, camera));
        let world = app.world();
        let request = *world.get::<TemporalUpscaling>(camera).unwrap();
        assert_eq!(request.upscaler, TemporalUpscaler::Dlss);
        assert_eq!(world.get::<MipBias>(camera).unwrap().0, mip_bias(50));
        assert_eq!(
            world.get::<TemporalJitter>(camera).unwrap().offset,
            jitter_offset(0, jitter_phase_count(50))
        );
        assert_eq!(*world.get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert!(world.get::<Smaa>(camera).is_none());
        assert!(
            TextureUsages::from(world.get::<Camera3d>(camera).unwrap().depth_texture_usages)
                .contains(TextureUsages::TEXTURE_BINDING)
        );
        select(&mut app, ui::Upscaler::Fsr1, 50);
        app.update();
        assert!(!requested(&app, camera));
    }

    #[test]
    fn shader_pack_cameras_never_request_temporal_upscaling() {
        let (mut app, camera) = app(Some(DLSS));
        select(&mut app, ui::Upscaler::Dlss, 100);
        app.update();
        assert!(requested(&app, camera));
        app.world_mut()
            .entity_mut(camera)
            .insert(ShaderPackRendering);
        app.update();
        assert!(!requested(&app, camera));
        app.world_mut()
            .entity_mut(camera)
            .remove::<ShaderPackRendering>();
        app.update();
        assert!(requested(&app, camera));
    }

    #[test]
    fn jitter_advances_each_frame_and_cuts_change_the_reset_epoch() {
        let (mut app, camera) = app(Some(DLSS));
        select(&mut app, ui::Upscaler::Dlss, 100);
        let phases = jitter_phase_count(100);
        let mut epoch = None;
        for frame in 0..u64::from(phases) + 2 {
            app.update();
            let world = app.world();
            assert_eq!(
                world.get::<TemporalJitter>(camera).unwrap().offset,
                jitter_offset(frame, phases)
            );
            let reset = world.get::<TemporalUpscaling>(camera).unwrap().reset_epoch;
            assert_eq!(*epoch.get_or_insert(reset), reset);
        }
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .reanchor_camera();
        app.update();
        let cut = app
            .world()
            .get::<TemporalUpscaling>(camera)
            .unwrap()
            .reset_epoch;
        assert_ne!(Some(cut), epoch);
        assert_eq!(cut, app.world().resource::<CameraCuts>().epoch());
    }

    #[test]
    fn only_the_advancing_jitter_is_rewritten_on_a_steady_frame() {
        let (mut app, camera) = app(Some(DLSS));
        select(&mut app, ui::Upscaler::Dlss, 75);
        app.update();
        let ticks = |app: &App| {
            let entity = app.world().entity(camera);
            [
                entity
                    .get_ref::<TemporalUpscaling>()
                    .unwrap()
                    .last_changed(),
                entity.get_ref::<MipBias>().unwrap().last_changed(),
                entity.get_ref::<Msaa>().unwrap().last_changed(),
                entity.get_ref::<Camera3d>().unwrap().last_changed(),
                entity.get_ref::<TemporalJitter>().unwrap().last_changed(),
            ]
        };
        let before = ticks(&app);
        app.update();
        let after = ticks(&app);
        assert_eq!(before[..4], after[..4]);
        assert_ne!(before[4], after[4]);
    }
}
