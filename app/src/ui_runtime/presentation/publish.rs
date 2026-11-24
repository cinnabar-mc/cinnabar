//! Per-frame HUD observation and publication.
use std::sync::Arc;

use crate::runtime::network::{PackReload, resource_packs::PackApplication};
use bevy::prelude::{DetectChanges, DetectChangesMut, Local, Projection, Transform};
use client_runtime::network::NetworkHandle;
use inventory::inventory_ledger::PlayerInventorySlot;
use {super::*, client_presentation::camera::CameraSettingsAuthority};

use launcher::menu::settings_options::SettingId;
use ui::mod_presentation::NametagVisibility;
#[cfg(test)]
mod camera_hand_tests;
mod commit;
mod cooldowns;
mod loading;
mod menu_icons;
mod roster;
use client_ui::ui_runtime::presentation::{
    ItemIconFrames, PendingUiPublication, PreviewCapture, capture_hud_frame_if_changed, nametags,
    player_preview,
};
pub(crate) use commit::publish_ui_runtime;

/// The UI authority captured before input enqueue, rendered once by `publish_ui_runtime`.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct PreparedUiPublication(pub(crate) Option<PendingUiPublication>);

pub(crate) fn observe_mount_jump_input(
    input: Res<client_state::SemanticInputSnapshot>,
    mut runtime: ResMut<UiRuntimeResource>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        runtime.set_mount_jump_held(input.phase(semantic_input::Action::Jump).held, now_millis)
            | runtime.set_player_list_held(input.phase(semantic_input::Action::PlayerList).held)
            | input
                .snapshot()
                .is_some_and(|snapshot| runtime.set_input_mode(snapshot.input_mode))
    });
}

/// Resources beyond Bevy's sixteen-parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct PublishExtras<'w, 's> {
    frame_poll: Res<'w, WorldStreamFramePoll>,
    #[cfg(not(target_arch = "wasm32"))]
    menu_runtime: Res<'w, client_shell::menu::MenuRuntime>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    #[cfg(target_arch = "wasm32")]
    skin: Res<'w, client_shell::player_skin::LocalPlayerSkin>,
    hand_rig: ResMut<'w, client_presentation::actor_publication::ActorFrameState>,
    collisions: Option<Res<'w, client_interaction::movement::PhysicsCollisionRegistries>>,
    profiler: Option<Res<'w, render_scene::RuntimeStageProfiler>>,
    actor_pipelines: Option<Res<'w, render_scene::ActorPipelineReadiness>>,
    warmed_pipelines: Option<Res<'w, render_scene::PipelineWarmupReadiness>>,
    server_camera: Option<Res<'w, client_presentation::camera::ServerCameraView>>,
    picked_actor: Option<Res<'w, crate::block_selection::CrosshairActor>>,
    nametag_cache: Local<'s, nametags::NametagTextCache>,
    fatal_error: ResMut<'w, crate::runtime::shutdown::FatalError>,
    actor_partial: Res<'w, client_presentation::actor_publication::ActorFramePartialTick>,
    local_frame: Res<'w, client_presentation::local_player::LocalPlayerFrameCarrier>,
    local_visibility: Res<'w, client_presentation::local_player::LocalAvatarVisibilityCarrier>,
    clock: Res<'w, client_presentation::environment::WorldClock>,
    weather: Res<'w, client_presentation::environment::WeatherState>,
    network: Res<'w, NetworkHandle<PackApplication>>,
    pack_reload: Option<Res<'w, PackReload>>,
    glint_settings: Option<ResMut<'w, render_scene::UiGlintSettings>>,
    item_use: Res<'w, client_interaction::item_use::ItemUseRuntime>,
    melee: Res<'w, client_interaction::melee::MeleeRuntime>,
    movement: Res<'w, client_interaction::movement::MovementTicker>,
    physics: Res<'w, client_interaction::movement::LocalPhysicsController>,
}

/// Observes UI authority and captures inventory, including this frame's outbound predictions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_ui_runtime(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut presentation: ResMut<UiPresentationResource>,
    mut prepared: ResMut<PreparedUiPublication>,
    visibility: Res<CaveVisibilityCache>,
    mut diagnostics_input: ResMut<VisibilityDiagnosticsInput>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
    render_queue: Res<ChunkRenderQueue>,
    upload_acknowledgements: Res<ChunkUploadAcknowledgements>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut client_world: ResMut<ClientWorld>,
    camera_settings: Res<CameraSettingsAuthority>,
    // The camera's Transform is this frame's; its GlobalTransform is propagated after Update.
    cameras: Query<(&Camera, &Transform, &Projection), With<Camera3d>>,
    time: Res<Time<Real>>,
    extras: PublishExtras,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
) {
    let PublishExtras {
        frame_poll,
        #[cfg(not(target_arch = "wasm32"))]
        menu_runtime,
        #[cfg(target_arch = "wasm32")]
        options,
        #[cfg(target_arch = "wasm32")]
        skin,
        mut hand_rig,
        collisions,
        profiler,
        actor_pipelines,
        warmed_pipelines,
        server_camera,
        picked_actor,
        mut nametag_cache,
        mut fatal_error,
        actor_partial,
        local_frame,
        local_visibility,
        clock,
        weather,
        network,
        pack_reload,
        glint_settings,
        item_use,
        melee,
        movement,
        physics,
    } = extras;
    #[cfg(not(target_arch = "wasm32"))]
    let (settings_snapshot, menu_skin, menu_visible, menu_scene, menu) = (
        menu_runtime.settings_snapshot(),
        menu_runtime.player_skin(),
        menu_runtime.is_visible(),
        Some(menu_runtime.screen()),
        Some(
            &*menu_runtime
                as &dyn client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene,
        ),
    );
    #[cfg(target_arch = "wasm32")]
    let (settings_snapshot, menu_skin, menu_visible, menu_scene, menu) = (
        (Arc::clone(&options.0), None),
        &*skin,
        false,
        None::<launcher::menu::MenuScreen>,
        None,
    );
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::UiPreparation));
    prepared.0 = None;
    // Every write below reports whether it changed anything, so an idle frame leaves the UI
    // runtime and presentation unchanged and publication may repeat the last frame.
    let mod_presentation = std::sync::Arc::clone(presentation.mod_ui().mod_presentation());
    let annotations = roster::annotations(&mod_presentation, client_world.stream.as_ref());
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        runtime.set_resource_pack_loading(
            pack_reload
                .as_deref()
                .is_some_and(PackReload::loading_screen),
        ) | runtime.session.player_roster.set_annotations(annotations)
            | runtime.advance_item_activation(now_millis)
            | runtime.advance_form_handoff(now_millis)
            | runtime.set_toast_lifetime(settings_snapshot.0.toast_lifetime_millis())
            | runtime.set_control_settings(Arc::clone(&settings_snapshot.0))
    });
    if let Some(mut glint_settings) = glint_settings {
        #[cfg(not(target_arch = "wasm32"))]
        let next = menu_runtime.ui_glint_settings();
        #[cfg(target_arch = "wasm32")]
        let next = render_scene::UiGlintSettings::default();
        glint_settings.set_if_neq(next);
    }
    let Ok(window) = windows.single() else {
        hand.clear();
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    if physical_size.contains(&0) {
        hand.clear();
        return;
    }
    let dpi_scale = match DpiScale::new(window.scale_factor()) {
        Ok(scale) => scale,
        Err(error) => {
            hand.clear();
            presentation.record_frame_failure(&UiPresentationError::Geometry(error));
            return;
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.set_safe_area(client_shell::platform_text::safe_area_insets(dpi_scale))
    });
    client_shell::ui_resources::update(&mut runtime, |runtime| runtime.expire_hud(now_millis));
    let restart = client_world.dimension_transfer.take_presentation_reset();
    // Startup readiness is not drawn: only the loading stage it settles reaches a frame.
    let loading_stage = presentation.loading_stage();
    loading::prepare_loading(
        &mut client_world,
        &mut fatal_error.0,
        presentation.bypass_change_detection(),
        &runtime,
        &mut diagnostics_input,
        &network,
        loading::LoadingObservation {
            restart,
            menu_visible,
            snapshot: visibility_diagnostics.snapshot(),
            visible_rendered: visibility.visible_rendered,
            cohort: frame_poll.cohort_progress,
            render_work_drained: render_queue.retained_len() == 0
                && upload_acknowledgements.is_empty(),
            pipelines_ready: actor_pipelines
                .as_ref()
                .is_none_or(|ready| ready.is_ready())
                && warmed_pipelines
                    .as_ref()
                    .is_none_or(|ready| ready.is_ready()),
            now: time.elapsed(),
        },
    );
    if presentation.loading_stage() != loading_stage {
        presentation.set_changed();
    }
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        runtime.expire_gameplay_effects(now_millis)
    });
    let stream = client_world.stream.as_ref();
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.set_preview_cosmetics(&menu_skin.cosmetics)
    });
    let menu_preview_skin = menu_skin.standard_skin();
    let preview_ready = presentation.menu_preview_skin_shown(&menu_preview_skin)
        || presentation.set_menu_preview_skin(&menu_preview_skin);
    let own_pixels = render_model::ActorSkinPixels {
        width: menu_skin.width,
        height: menu_skin.height,
        rgba8: menu_skin.rgba8.clone(),
    };
    let skin = if menu_visible {
        player_preview::local_preview_skin(None, &own_pixels)
    } else {
        player_preview::local_preview_skin(
            stream.map(|stream| stream.authority().local_player_ui()),
            &own_pixels,
        )
    };
    let dressing_room =
        menu_visible && menu_scene == Some(launcher::menu::MenuScreen::DressingRoom);
    let pose = if dressing_room {
        player_preview::PlayerPreviewPose::default()
    } else {
        player_preview::PlayerPreviewPose::of_local_player(
            stream.map(|stream| stream.authority().local_player_ui()),
        )
    };
    let settings = Arc::clone(&settings_snapshot.0);
    let show_custom_item_models = settings.value(SettingId::ShowCustomItemModels) != 0;
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.set_preview_equipment_assets(
            client_world.entity_assets.clone(),
            client_world
                .pack_entities
                .as_ref()
                .map(|pack| Arc::clone(&pack.assets)),
        ) | presentation.set_preview_pack_equipment(
            client_world
                .pack_entities
                .as_ref()
                .and_then(|pack| pack.equipment.clone()),
        ) | presentation.set_preview_player_layers(client_world.pack_entities.clone())
            | presentation.set_player_preview_custom_item_models(show_custom_item_models)
    });
    // The model wears the local player's armor and held item. Its dressing is drawn only with
    // the model, which counts its own revision; other changes are reported.
    if dressing_room {
        presentation.set_player_preview_gear([None; 4], None);
    } else {
        client_shell::ui_resources::update(&mut presentation, |presentation| {
            presentation.dress_player_preview(
                &player_runtime,
                &runtime,
                stream.map(|stream| stream.authority().inventory_ui()),
                actor_animation::AttachableAnimationInput {
                    frame_alpha: actor_partial.0,
                    delta_seconds: Some(time.delta_secs()),
                    ..Default::default()
                },
                |stack| {
                    client_world
                        .stream
                        .as_ref()?
                        .authority()
                        .canonical_item_stack(stack)
                },
            )
        });
    }
    let doll_state = client_world
        .stream
        .as_ref()
        .and_then(|stream| observe_paper_doll(stream, &runtime, &player_runtime, &physics));
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.observe_paper_doll(now_millis, doll_state)
    });
    // The HUD doll keeps animating under an open screen, beside that screen's own doll.
    let hud_doll = presentation
        .hud_frame()
        .paper_doll_requested(player_runtime.facts.player_game_mode(), &settings)
        && settings.value(SettingId::HideHud) == 0
        && presentation.loading_stage().is_none()
        && !menu_visible;
    let emote_preview = runtime
        .emotes()
        .playback()
        .map(|playback| (playback.emote, playback.elapsed(now_millis)))
        .or_else(|| {
            runtime
                .emotes()
                .is_open()
                .then(|| {
                    let slots = runtime.emotes().slots();
                    let selected = runtime
                        .emotes()
                        .selected_slot()
                        .and_then(|slot| slots[slot])
                        .or_else(|| slots.iter().copied().flatten().next())?;
                    Some((selected, now_millis as f64 / 1_000.0))
                })
                .flatten()
        });
    // The live capture writes only the player model, which advances its own revision that a
    // frame reads when it draws the model.
    let live = presentation.bypass_change_detection();
    live.begin_live_capture();
    // A screen's live renderer draws the same animated actor as the HUD doll.
    if hud_doll
        || (live.has_preview_player_layers() && !menu_visible)
        || runtime.emotes().is_open()
        || (live.live_renderer_shown() && !menu_visible)
    {
        live.set_cosmetic_motion(hand_rig.cosmetic_motion());
        live.set_hud_glide(
            client_world
                .stream
                .as_ref()
                .and_then(|stream| stream.authority().local_player_ui().actor())
                .and_then(|actor| {
                    client_presentation::presentation::actors::glide_rotation(
                        actor,
                        actor_partial.0,
                    )
                }),
        );
        live.capture_hud_player_with_emote(
            client_world
                .stream
                .as_ref()
                .map(|stream| stream.authority().local_player_ui()),
            doll_state.is_some_and(|state| state.swimming),
            emote_preview,
        );
    }
    let overlays = client_presentation::presentation::visibility::GameplayOverlayVisibility::new(
        settings.value(SettingId::HideHud) != 0,
        settings.value(SettingId::HideHand) != 0,
        player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator),
    );
    let hide_hand = !overlays.hand;
    // The paper doll shows in the inventory and menus; the CPU hands only while no GPU hand rig.
    let first_person = client_presentation::camera::hand_first_person(
        Some(&*camera_settings),
        server_camera.as_deref(),
    );
    let java_held_item = camera_settings.feel().java_animations
        && stream
            .and_then(|stream| {
                stream
                    .authority()
                    .actor_rig(stream.local_player_runtime_id())
            })
            .map_or_else(
                || {
                    player_runtime
                        .selected_stack_snapshot()
                        .is_some_and(|selected| {
                            matches!(selected.state, PlayerInventorySlot::Present(_))
                        })
                },
                |rig| rig.java_equipped.is_some(),
            );
    let preview = PreviewCapture {
        ready: preview_ready,
        skin,
        pose,
        shown: runtime.inventory_open() || menu_visible || hud_doll || runtime.emotes().is_open(),
        hands: first_person && !hide_hand && !hand_rig.hand_is_active() && !java_held_item,
    };
    // It observes only an open station's block.
    let station_open = {
        let ledger = runtime.inventory_ledger(&player_runtime);
        ledger.window_kind().is_some() && ledger.window_position().is_some()
    };
    client_ui::ui_runtime::presentation::forms::observe_station_block(
        &player_runtime,
        if station_open {
            &mut runtime
        } else {
            runtime.bypass_change_detection()
        },
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.authority().inventory_ui()),
        |position| {
            station_triggered(
                client_world.stream.as_ref()?,
                collisions.as_deref()?,
                position,
            )
        },
        |position| {
            beacon_level(
                client_world.stream.as_ref()?,
                collisions.as_deref()?,
                position,
            )
        },
        now_millis,
    );
    let icon_frames = ItemIconFrames(std::array::from_fn(|slot| {
        client_world.stream.as_ref().and_then(|stream| {
            item_use.inventory_animation_frame(
                &player_runtime,
                stream,
                &runtime,
                slot as u8,
                movement.completed_tick(),
            )
        })
    }));
    // When the local player's first-person rig is drawing near-camera, it owns the hand; the
    // static empty-hand scene and the HUD's CPU hand/item carriers are retired so nothing
    // double-draws.
    let hud_first_person = first_person && !hide_hand;
    // Writers mark the runtimes changed only when they change them, and publication marks
    // neither, so unchanged ticks mean the capture would store nothing new from them.
    let runtimes_unchanged = !player_runtime.is_changed() && !runtime.is_changed();
    let capture = capture_hud_frame_if_changed(
        &player_runtime,
        runtime.bypass_change_detection(),
        presentation.bypass_change_detection(),
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.authority().inventory_ui()),
        hud_first_person,
        now_millis,
        icon_frames,
        runtimes_unchanged,
    );
    if capture.runtime_changed {
        runtime.set_changed();
    }
    if capture.presentation_changed {
        presentation.set_changed();
    }
    let item_icons = capture.icons;
    let hotbar_cooldowns = cooldowns::hotbar_cooldowns(
        &player_runtime,
        &runtime,
        client_world.stream.as_ref(),
        &item_use,
        &melee,
        movement.completed_tick(),
    );
    // Floored feet position and day count for the HUD's position and days-played text.
    let player_block = local_frame.snapshot().map(|frame| {
        let feet = frame.feet();
        [feet.x, feet.y, feet.z].map(|axis| axis.floor() as i32)
    });
    let locator_view = cameras
        .single()
        .ok()
        .and_then(|(_, transform, projection)| {
            let Projection::Perspective(projection) = projection else {
                return None;
            };
            Some(ui_screens::hud_renderers::LocatorView {
                position: transform.translation.to_array(),
                forward: transform.forward().to_array(),
                right: transform.right().to_array(),
                vertical_fov: projection.fov,
            })
        });
    let dimension = client_world
        .stream
        .as_ref()
        .map_or(0, |stream| stream.current_dimension());
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        runtime.observe_locator_actors(dimension, |id| {
            client_world
                .stream
                .as_ref()?
                .authority()
                .actor_by_unique_id(id)?
                .interpolated_position(actor_partial.0.clamp(0.0, 1.0))
        })
    });
    let days_played = Some(
        (client_presentation::environment::visual_world_time(*clock, time.elapsed_secs_f64())
            / f64::from(world::TICKS_PER_DAY))
        .floor(),
    );
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        let frame = presentation.hud_frame_mut();
        let mut changed = store(&mut frame.hotbar_cooldowns, hotbar_cooldowns);
        changed |= store(&mut frame.player_block, player_block);
        changed |= store(&mut frame.thunderstorm, weather.lightning_level() > 0.0);
        changed |= store(&mut frame.locator_view, locator_view);
        changed |= store(
            &mut frame.death_screen,
            menu_visible && menu_scene == Some(launcher::menu::MenuScreen::Death),
        );
        changed |= store(&mut frame.dimension, dimension);
        changed |= store(&mut frame.days_played, days_played);
        changed | store(&mut frame.hand_rig_active, hand_rig.hand_is_active())
    });
    if hand_rig.hand_is_active() {
        hand.use_animated_rig();
    } else {
        hand.observe(
            &player_runtime,
            &runtime,
            &client_world,
            hud_first_person,
            hide_hand
                || !presentation.renders_game_behind(&player_runtime, &runtime, menu)
                || presentation.loading_stage().is_some(),
            physical_size,
        );
    }
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        runtime
            .session
            .player_roster
            .set_show_badges(settings.value(SettingId::CinnabarBadges) != 0)
    });
    let show_names = settings.value(SettingId::IngamePlayerNames) != 0;
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.prepare_nametag_layout(&runtime)
    });
    // Mod nametag rules decide each tag after native eligibility; with none, every tag is
    // native, and none draws while the HUD is hidden.
    let hud_hidden = !overlays.nametags;
    let own_visibility = client_world.stream.as_ref().and_then(|stream| {
        let authority = stream.authority();
        let actor = authority.actor(authority.local_player_runtime_id())?;
        Some(mod_presentation.nametag_visibility(
            &client_presentation::actor_publication::mod_actor_facts(authority, actor),
            hud_hidden,
        ))
    });
    // An annotation on the local player shows its own tag, as other annotated players' tags do.
    let annotated_self = client_world.stream.as_ref().is_some_and(|stream| {
        mod_presentation
            .nametag(stream.authority().local_player_runtime_id(), true)
            .is_some()
    });
    let show_self_tag = (own_visibility == Some(NametagVisibility::Shown)
        || annotated_self
        || settings.value(SettingId::F5SelfVisuals) != 0)
        && !first_person;
    // Name tags draw outside the UI frame, so their bounds and anchors change no frame input.
    let nametag_presentation = presentation.bypass_change_detection();
    let nametags = client_world
        .stream
        .as_ref()
        .filter(|_| {
            !hud_hidden || (!menu_visible && mod_presentation.shows_nametags_with_hidden_hud())
        })
        .zip(cameras.single().ok().map(|(_, transform, projection)| {
            (
                GlobalTransform::from(*transform),
                projection.get_clip_from_view(),
            )
        }))
        .map(|(stream, (transform, clip_from_view))| {
            let authority = stream.authority();
            let nametag_view = nametags::NametagView {
                camera_position: transform.translation(),
                camera_forward: *transform.forward(),
            };
            let clip_from_world = clip_from_view * transform.to_matrix().inverse();
            let rotation = transform.rotation();
            let mut admit = |tag: &mut nametags::NametagAnchor| {
                let Some(facts) = authority.actor(tag.runtime_id()).map(|actor| {
                    client_presentation::actor_publication::mod_actor_facts(authority, actor)
                }) else {
                    return false;
                };
                if mod_presentation.nametag_visibility(&facts, hud_hidden)
                    == NametagVisibility::Hidden
                {
                    return false;
                }
                let player = facts.player;
                tag.set_cinnabar_badge(
                    authority
                        .actor_player_roster_identity(tag.runtime_id())
                        .is_some_and(|(xuid, uuid)| {
                            runtime.session.player_roster.has_badge(xuid, uuid)
                        }),
                );
                tag.set_annotation(mod_presentation.nametag(tag.runtime_id(), player).cloned());

                nametag_presentation.nametag_intersects_view(tag, clip_from_world, rotation)
            };
            let mut tags = nametags::project_nametags(
                &mut nametag_cache,
                runtime.scoreboards(),
                stream.authority().nametag_ui(),
                nametag_view,
                picked_actor.as_deref().and_then(|picked| picked.0),
                actor_partial.0,
                show_names,
                &mut admit,
            );
            if show_self_tag && show_names {
                let authority = stream.authority();
                if let Some(visibility) = local_visibility.snapshot().filter(|visibility| {
                    visibility.visible()
                        && visibility.runtime_id() == authority.local_player_runtime_id()
                        && visibility.session_generation().get() == clock.session_generation()
                }) && let Some(actor) = authority.actor(authority.local_player_runtime_id())
                    && let Some(name) = authority.actor_name_tag(actor.unique_id)
                    && let Some(mut tag) = nametags::extract_nametag_at_feet(
                        actor,
                        nametag_view,
                        None,
                        name,
                        (!authority.remote_actors().any(|rider| {
                            authority.ridden_unique_id(rider.unique_id) == Some(actor.unique_id)
                        }))
                        .then_some(runtime.scoreboards()),
                        visibility.feet(),
                    )
                    && admit(&mut tag)
                {
                    tags.push(tag);
                }
            }

            tags
        })
        .unwrap_or_default();
    let hud_camera = cameras.single().ok().map(|(_, transform, projection)| {
        (projection.get_clip_from_view() * GlobalTransform::from(*transform).to_matrix().inverse())
            .to_cols_array()
    });
    let actor_stream = client_world.stream.as_ref();
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation
            .mod_ui_mut()
            .set_hud_projection(hud_camera, &|runtime_id| {
                let actor = actor_stream?.authority().actor(runtime_id)?;
                Some(mod_ui::ProjectedActor {
                    feet: actor.interpolated_position(actor_partial.0.clamp(0.0, 1.0))?,
                    player: matches!(actor.kind, protocol::ActorKind::Player { .. }),
                })
            })
    });
    let nametag_presentation = presentation.bypass_change_detection();
    nametag_presentation.set_nametag_local_actor(
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.authority().local_player_runtime_id()),
    );
    nametag_cache.recycle_anchors(nametag_presentation.set_nametag_anchors(nametags));
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.set_chat_settings_snapshot(settings_snapshot)
    });
    #[cfg(not(target_arch = "wasm32"))]
    let menu_view = menu_visible.then(|| {
        let mut view = menu_runtime.view();
        presentation.sync_menu_artwork_view(&view);
        for server in view.featured.iter_mut() {
            server.icon = presentation.menu_artwork_icon(&server.image_path);
        }
        menu_icons::install(&presentation, &mut view);
        view.profile_icon = presentation.player_preview_icon();
        view
    });
    #[cfg(target_arch = "wasm32")]
    let menu_view = None;
    client_shell::ui_resources::update(&mut presentation, |presentation| {
        presentation.set_menu_view(menu_view)
            | presentation.refresh_scoreboard_owner_names(
                runtime.scoreboards(),
                client_world
                    .stream
                    .as_ref()
                    .map(|stream| stream.authority().nametag_ui()),
            )
    });
    client_shell::ui_resources::update(&mut runtime, |runtime| {
        presentation.publish_scene_inputs(runtime)
    });
    prepared.0 = Some(PendingUiPublication {
        inventory: runtime.capture_presentation_inventory(&player_runtime),
        preview,
        item_icons,
        now_millis,
        physical_size,
        dpi_scale,
    });
}

/// Replaces `slot` with `value`; whether it differed.
fn store<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        return false;
    }
    *slot = value;
    true
}

/// Captures predicted local movement and authoritative armor without server echo latency.
fn observe_paper_doll(
    stream: &chunk_pipeline::WorldStream,
    runtime: &client_ui::ui_runtime::UiRuntime,
    player_runtime: &client_state::PlayerRuntime,
    physics: &client_interaction::movement::LocalPhysicsController,
) -> Option<client_ui::ui_runtime::presentation::paper_doll::State> {
    use inventory::inventory_ledger::InventoryTarget;
    let actor = stream.authority().actor(stream.local_player_runtime_id())?;
    use protocol_ids::ActorFlag;
    let flag = |flag: ActorFlag| actor_animation::AnimatedActor::flag(actor, flag.wire_value());
    let (sneaking, sprinting) = physics
        .latest_sneak_sprint()
        .unwrap_or((flag(ActorFlag::Sneaking), flag(ActorFlag::Sprinting)));
    Some(client_ui::ui_runtime::presentation::paper_doll::State {
        sneaking,
        sprinting,
        in_water: physics.in_water(),
        swimming: physics.mode() == sim::MovementMode::Swimming || flag(ActorFlag::Swimming),
        crawling: flag(ActorFlag::Crawling),
        flying: physics.mode() == sim::MovementMode::Flying,
        gliding: physics.mode() == sim::MovementMode::Gliding || flag(ActorFlag::Gliding),
        emoting: flag(ActorFlag::Emoting) || runtime.emotes().playback().is_some(),
        armor: std::array::from_fn(|slot| {
            runtime
                .inventory_ledger(player_runtime)
                .target_stack(InventoryTarget::Armor(slot as u8))
                .map_or(0, |stack| stack.network_id)
        }),
    })
}

/// Resolves the crafter block's triggered state at the existing UI capture boundary.
fn station_triggered(
    stream: &chunk_pipeline::WorldStream,
    collisions: &client_interaction::movement::PhysicsCollisionRegistries,
    position: [i32; 3],
) -> Option<bool> {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let runtime_id = world.primary_runtime_id(position).ok()?;
    assets::block_presentation_state::pinned_block_presentation_states()
        .get(mode, runtime_id)
        .crafter_triggered
}

/// Counts the beacon pyramid's layers of base blocks from the loaded world.
fn beacon_level(
    stream: &chunk_pipeline::WorldStream,
    collisions: &client_interaction::movement::PhysicsCollisionRegistries,
    position: [i32; 3],
) -> Option<u8> {
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let states = assets::block_presentation_state::pinned_block_presentation_states();
    // Unloaded terrain leaves the level unknown rather than reading as a broken pyramid.
    client_ui::ui_runtime::presentation::forms::beacon_pyramid_level(position, |block| {
        world
            .primary_runtime_id(block)
            .ok()
            .map(|runtime_id| states.get(mode, runtime_id).beacon_base)
    })
}

#[cfg(test)]
mod item_icons;
