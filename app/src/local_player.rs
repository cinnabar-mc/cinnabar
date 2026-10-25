use bevy::{
    ecs::system::SystemParam,
    prelude::{Entity, Local, Res, ResMut, Single, Transform, Vec3, With},
};
use simulation_clock::identity::SessionGeneration;

use {
    crate::runtime::world::ClientWorld,
    client_interaction::movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    client_presentation::camera::{CameraSettingsAuthority, FlyCamera},
    client_presentation::environment::WorldClock,
};

use client_presentation::actor_publication::ActorFramePartialTick;
use client_presentation::local_player::{CameraPose, LocalPlayerFrameCarrier, LocalViewPose};

const LOCAL_PLAYER_POINT_LIGHT_HEIGHT_ABOVE_FEET: f32 = 1.0;
const MAX_ADDITIONAL_ITEM_POINT_LIGHTS: usize = 7;
const RENDER_DISTANCE_FADE_RADIUS_MULTIPLIER: f32 = 2.0;
const MIN_RENDER_DISTANCE_FADE_BLOCKS: f32 = 8.0;

#[derive(SystemParam)]
pub(crate) struct CameraPublicationContext<'w> {
    clock: Res<'w, WorldClock>,
    physics: Res<'w, LocalPhysicsController>,
    receipt: ResMut<'w, client_presentation::local_player_camera_receipt::CameraPublicationAttempt>,
}

/// Resolves one presentation pose before camera, actor and particle consumers read it.
pub(crate) fn resolve_local_player_presentation(
    mut world: ResMut<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    settings: Res<CameraSettingsAuthority>,
    mut view: ResMut<LocalViewPose>,
    physics: Res<LocalPhysicsController>,
) {
    view.resolve_presentation(
        world.stream.as_mut(),
        &*collisions,
        settings.perspective(),
        physics.render_feet_position(),
    );
}

/// Borrows world and completed physics facts for presentation publication.
pub(crate) fn resolve_camera_pose(
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    published: ResMut<CameraPose>,
    publication: CameraPublicationContext,
    camera_transform: Single<(Entity, &mut Transform), With<FlyCamera>>,
) {
    let CameraPublicationContext {
        clock,
        physics,
        receipt,
    } = publication;
    let observed = client_presentation::local_player::CameraPublicationContext {
        clock: client_presentation::observations::SessionObservation(SessionGeneration::new(
            clock.session_generation(),
        )),
        physics: &*physics,
        receipt,
    };
    client_presentation::local_player::resolve_camera_pose(
        client_presentation::observations::WorldObservation {
            stream: client_world.stream.as_ref(),
        },
        &*collisions,
        settings,
        view,
        published,
        observed,
        camera_transform,
    );
}

/// Borrows world and completed physics facts for presentation publication.
pub(crate) fn publish_local_player_frame(
    client_world: Res<ClientWorld>,
    publication: CameraPublicationContext,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    camera: Res<CameraPose>,
    carrier: ResMut<LocalPlayerFrameCarrier>,
) {
    let CameraPublicationContext {
        clock,
        physics,
        receipt,
    } = publication;
    let observed = client_presentation::local_player::CameraPublicationContext {
        clock: client_presentation::observations::SessionObservation(SessionGeneration::new(
            clock.session_generation(),
        )),
        physics: &*physics,
        receipt,
    };
    client_presentation::local_player::publish_local_player_frame(
        client_presentation::observations::WorldObservation {
            stream: client_world.stream.as_ref(),
        },
        observed,
        settings,
        view,
        camera,
        carrier,
    );
}

/// Returns the local body center used by the point light, independent of camera boom.
pub(crate) fn local_player_point_light_position(
    carrier: &LocalPlayerFrameCarrier,
) -> Option<[f32; 3]> {
    let position =
        carrier.snapshot()?.feet() + Vec3::Y * LOCAL_PLAYER_POINT_LIGHT_HEIGHT_ABOVE_FEET;
    position.is_finite().then_some(position.to_array())
}

/// Keeps the GPU point light anchored to the published local-player feet position.
pub(crate) fn sync_world_point_light_from_local_player(
    carrier: Res<LocalPlayerFrameCarrier>,
    world: Res<ClientWorld>,
    player: Option<Res<client_state::PlayerRuntime>>,
    ui: Option<Res<client_shell::ui_resources::UiRuntimeResource>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    light: Option<ResMut<render_scene::WorldPointLight>>,
) {
    let Some(mut light) = light else {
        return;
    };
    let enabled = menu.as_deref().is_some_and(|menu| {
        menu.settings_snapshot()
            .0
            .value(launcher::menu::settings_options::SettingId::DynamicLights)
            != 0
    });
    if light.enabled != enabled {
        light.enabled = enabled;
    }
    let position = local_player_point_light_position(&carrier).unwrap_or([0.0; 3]);
    let profile = world.stream.as_ref().and_then(|stream| {
        let main_hand = player
            .as_deref()
            .and_then(|player| player.selected_stack())
            .and_then(|item| item_light_profile(stream, item.block_runtime_id));
        let off_hand = player.as_deref().and_then(|player| {
            ui.as_deref()
                .and_then(|ui| ui.local_offhand_stack(player))
                .and_then(|item| item_light_profile(stream, item.block_runtime_id))
        });
        [main_hand, off_hand]
            .into_iter()
            .flatten()
            .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
    });
    let local = profile.map_or_else(
        || render_scene::WorldPointLightSource {
            position,
            ..Default::default()
        },
        |profile| render_scene::WorldPointLightSource {
            position,
            intensity: profile.intensity,
            radius: profile.radius,
        },
    );
    if light.local != local {
        light.local = local;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ItemLightProfile {
    intensity: f32,
    radius: f32,
}

fn item_light_profile(
    stream: &chunk_pipeline::WorldStream,
    item_block: i32,
) -> Option<ItemLightProfile> {
    client_interaction::block_use::held_block_store_id(stream, item_block)
        .and_then(|runtime_id| {
            stream
                .runtime_assets()
                .resolve_known(stream.network_id_mode(), runtime_id)
        })
        .and_then(|block| item_light_profile_from_emission(block.light_properties().emission()))
}

fn item_light_profile_from_emission(emission: u8) -> Option<ItemLightProfile> {
    let level = emission.min(15);
    (level > 0).then(|| {
        let normalized = f32::from(level) / 15.0;
        ItemLightProfile {
            intensity: normalized,
            radius: 2.0 + normalized * 6.0,
        }
    })
}

/// Publishes nearest remote-player and dropped-item lights for the vanilla shader.
pub(crate) fn sync_remote_item_point_lights(
    world: Res<ClientWorld>,
    partial_tick: Res<ActorFramePartialTick>,
    mut scratch: Local<ItemLightScratch>,
    point_light: Option<ResMut<render_scene::WorldPointLight>>,
) {
    let Some(mut point_light) = point_light else {
        return;
    };
    if !point_light.enabled {
        point_light.additional = [render_scene::WorldPointLightSource::default(); 7];
        return;
    }
    let Some(stream) = world.stream.as_ref() else {
        point_light.additional = [render_scene::WorldPointLightSource::default(); 7];
        return;
    };
    let ItemLightScratch {
        candidates,
        dropped_items,
    } = &mut *scratch;

    let origin = Vec3::from_array(point_light.local.position);
    let render_distance = stream.render_distance_blocks();
    if !render_distance.is_finite() || render_distance <= 0.0 {
        point_light.additional = [render_scene::WorldPointLightSource::default(); 7];
        return;
    }
    let render_distance_squared = render_distance * render_distance;
    let alpha = if partial_tick.0.is_finite() {
        partial_tick.0.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let authority = stream.authority();
    candidates.clear();
    for actor in authority.present_remote_players() {
        let profile = [
            protocol::ActorHandedness::Right,
            protocol::ActorHandedness::Left,
        ]
        .into_iter()
        .map(|hand| {
            authority
                .actor_equipment_in_hand(actor.runtime_id, hand)
                .and_then(|item| item_light_profile(stream, item.item.identity.block_runtime_id))
        })
        .flatten()
        .max_by(|left, right| left.intensity.total_cmp(&right.intensity));
        let Some(profile) = profile else {
            continue;
        };
        let position = if actor.teleported {
            Vec3::from_array(actor.position)
        } else {
            Vec3::from_array(actor.previous_pose.position)
                .lerp(Vec3::from_array(actor.position), alpha)
        } + Vec3::Y;
        if !position.is_finite() {
            continue;
        }
        let distance_squared = origin.distance_squared(position);
        if !distance_squared.is_finite() || distance_squared > render_distance_squared {
            continue;
        }
        let fade =
            point_light_render_fade(distance_squared.sqrt(), render_distance, profile.radius);
        if fade <= 0.0 {
            continue;
        }
        push_nearest_item_point_light(
            candidates,
            distance_squared,
            actor.runtime_id,
            render_scene::WorldPointLightSource {
                position: position.to_array(),
                intensity: profile.intensity * fade,
                radius: profile.radius,
            },
        );
    }
    authority.dropped_items_into(alpha, dropped_items);
    for dropped in dropped_items.iter() {
        if dropped.render_scale <= 0.0 {
            continue;
        }
        let Some(profile) = item_light_profile(stream, dropped.item.identity.block_runtime_id)
        else {
            continue;
        };
        let mut position = Vec3::from_array(dropped.position);
        position.y += 0.25;
        if !position.is_finite() {
            continue;
        }
        let distance_squared = origin.distance_squared(position);
        if !distance_squared.is_finite() || distance_squared > render_distance_squared {
            continue;
        }
        let fade =
            point_light_render_fade(distance_squared.sqrt(), render_distance, profile.radius);
        if fade <= 0.0 {
            continue;
        }
        push_nearest_item_point_light(
            candidates,
            distance_squared,
            dropped.runtime_id,
            render_scene::WorldPointLightSource {
                position: position.to_array(),
                intensity: profile.intensity * fade,
                radius: profile.radius,
            },
        );
    }
    let mut additional = [render_scene::WorldPointLightSource::default(); 7];
    for (slot, (_, _, source)) in candidates.iter().enumerate() {
        additional[slot] = *source;
    }
    if point_light.additional != additional {
        point_light.additional = additional;
    }
}

fn point_light_render_fade(distance: f32, render_distance: f32, radius: f32) -> f32 {
    if !distance.is_finite()
        || !render_distance.is_finite()
        || !radius.is_finite()
        || distance < 0.0
        || radius <= 0.0
        || render_distance <= 0.0
        || distance >= render_distance
    {
        return 0.0;
    }
    let fade_width = (radius * RENDER_DISTANCE_FADE_RADIUS_MULTIPLIER)
        .max(MIN_RENDER_DISTANCE_FADE_BLOCKS)
        .min(render_distance);
    let fade_start = render_distance - fade_width;
    if distance <= fade_start {
        return 1.0;
    }
    let t = ((distance - fade_start) / fade_width).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

fn push_nearest_item_point_light(
    candidates: &mut Vec<(f32, u64, render_scene::WorldPointLightSource)>,
    distance_squared: f32,
    runtime_id: u64,
    source: render_scene::WorldPointLightSource,
) {
    let index = candidates
        .binary_search_by(|candidate| {
            candidate
                .0
                .total_cmp(&distance_squared)
                .then_with(|| candidate.1.cmp(&runtime_id))
        })
        .unwrap_or_else(|index| index);
    if index >= MAX_ADDITIONAL_ITEM_POINT_LIGHTS {
        return;
    }
    candidates.insert(index, (distance_squared, runtime_id, source));
    if candidates.len() > MAX_ADDITIONAL_ITEM_POINT_LIGHTS {
        candidates.pop();
    }
}

#[derive(Default)]
pub(crate) struct ItemLightScratch {
    candidates: Vec<(f32, u64, render_scene::WorldPointLightSource)>,
    dropped_items: Vec<client_world::DroppedItemView>,
}

#[cfg(test)]
mod point_light_tests {
    #[test]
    fn item_lights_fade_smoothly_to_the_configured_view_edge() {
        let render_distance = 128.0;
        assert_eq!(
            super::point_light_render_fade(100.0, render_distance, 8.0),
            1.0
        );
        assert_eq!(
            super::point_light_render_fade(120.0, render_distance, 8.0),
            0.5
        );
        assert_eq!(
            super::point_light_render_fade(128.0, render_distance, 8.0),
            0.0
        );
        assert_eq!(
            super::point_light_render_fade(129.0, render_distance, 8.0),
            0.0
        );
    }

    #[test]
    fn item_light_candidates_keep_only_the_seven_nearest_sources() {
        let mut candidates = Vec::new();
        for runtime_id in (0..10).rev() {
            super::push_nearest_item_point_light(
                &mut candidates,
                runtime_id as f32,
                runtime_id,
                render_scene::WorldPointLightSource {
                    intensity: 1.0,
                    ..Default::default()
                },
            );
        }
        assert_eq!(candidates.len(), super::MAX_ADDITIONAL_ITEM_POINT_LIGHTS);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.1)
                .collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
    }

    #[test]
    fn item_emission_sets_its_own_strength_and_radius() {
        assert_eq!(super::item_light_profile_from_emission(0), None);
        let medium = super::item_light_profile_from_emission(7).unwrap();
        assert_eq!(medium.intensity, 7.0 / 15.0);
        assert!((medium.radius - 4.8).abs() < 0.0001);
        let strongest = super::item_light_profile_from_emission(15).unwrap();
        assert_eq!(strongest.intensity, 1.0);
        assert_eq!(strongest.radius, 8.0);
        assert_eq!(
            super::item_light_profile_from_emission(255),
            super::item_light_profile_from_emission(15)
        );
    }
}
