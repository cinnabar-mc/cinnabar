//! Applies extension observations through the shared gameplay and presentation owners.

use super::*;

/// Native presentation facts and retained observers available to one mod frame.
#[derive(SystemParam)]
pub(super) struct ModObservation<'w, 's> {
    weather: Option<ResMut<'w, crate::environment::VisualWeatherOverride>>,
    action_filter: Local<'s, semantic_input::action_filter::ActionFilterState>,
    action_scope: Local<'s, Option<(u64, i32)>>,
    packets: Option<Res<'w, packets::PacketMods>>,
    watcher: Option<Res<'w, registration::Watcher>>,
    render_scene: Option<ResMut<'w, render_scene::ModRenderScene>>,
    drawing: Option<ResMut<'w, packet_render::Drawing>>,
    /// The presented camera that `read-camera` reports.
    cameras: Query<
        'w,
        's,
        (&'static GlobalTransform, &'static Projection),
        With<client_presentation::camera::FlyCamera>,
    >,
    frame_rate: Local<'s, presentation_facts::FrameRate>,
    module_observer: Local<'s, module_observations::ModuleObserver>,
    terrain: Local<'s, terrain::TerrainSampling>,
    actor_style: Option<ResMut<'w, client_presentation::actor_publication::ActorPresentationStyle>>,
    relayed: Option<Res<'w, packet_delay::RealPositionSnapshot>>,
    /// The damage shake that `read-damage-shake` reports.
    damage_shake: Option<Res<'w, client_presentation::camera::actor_effects::PresentedDamageShake>>,
    native_camera: Local<'s, native_camera::NativeCamera>,
    bindings: Local<'s, bindings::Bindings>,
    input_settings: Option<Res<'w, client_state::RuntimeSettings>>,
}

#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(super) fn drive_mod(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    extension: Option<ResMut<ModRuntime>>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
    windows: Query<(&Window, Option<&CursorOptions>), With<PrimaryWindow>>,
    mut ui: ResMut<UiRuntimeResource>,
    menu: Option<Res<MenuRuntime>>,
    mut presentation: ResMut<UiPresentationResource>,
    time_override: Option<ResMut<VisualTimeOverride>>,
    mut gameplay: gameplay::GameplayContext,
    interaction: Option<ResMut<interaction::ModInteraction>>,
    outputs: ModOutputs,
    mut screen_input: screens::ScreenInput,
    time: Res<Time<Real>>,
    observation: ModObservation,
) {
    let ModObservation {
        mut weather,
        mut action_filter,
        mut action_scope,
        packets,
        watcher,
        mut render_scene,
        mut drawing,
        cameras,
        mut frame_rate,
        mut module_observer,
        mut terrain,
        mut actor_style,
        relayed,
        damage_shake,
        mut native_camera,
        mut bindings,
        input_settings,
    } = observation;
    let now = time.elapsed_secs_f64();
    let (network, mut camera, cues, mut item_use) = outputs;
    if let Some(weather) = weather.as_deref_mut() {
        weather.0 = None;
    }
    if let Some(policy) = item_use.as_deref_mut() {
        policy.pulse = false;
    }
    let user_freelook = gameplay
        .input
        .as_ref()
        .and_then(|input| input.snapshot())
        .is_some_and(|input| input.phases[semantic_input::Action::Freelook as usize].held);
    let focused = windows.single().is_ok_and(|(window, _)| window.focused);
    let absorbed = client_shell::screen_policy::absorbs_input(
        &player_runtime,
        Some(&ui),
        menu.as_deref(),
        Some(&presentation),
    );
    let captured = windows.single().is_ok_and(|(window, cursor)| {
        cursor.is_some_and(|cursor| {
            crate::camera::mouse_input_active(window, cursor, focus.as_deref(), driven.is_some())
        })
    });
    let movement_allowed = extension.as_ref().is_some_and(|runtime| {
        !runtime.suspended
            && (0..runtime.hosts.host_count()).any(|index| {
                runtime.hosts.host(index).is_active() && runtime.hosts.host(index).grants().movement
            })
    });
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed, &ui);
    let menu_open = menu.as_deref().is_some_and(MenuRuntime::is_visible);
    screen_input.modifiers(focused && !menu_open);
    let (Some(mut extension), Some(mut time_override), Some(mut interaction)) =
        (extension, time_override, interaction)
    else {
        action_filter::inactive(
            &mut action_filter,
            gameplay.input.as_deref_mut(),
            time.elapsed().as_micros(),
        );
        *action_scope = None;
        let output = exchange_native_frame(
            packets.as_deref(),
            &mut native_camera,
            &gameplay,
            captured && !absorbed,
            &ui,
            (time.delta_secs(), now),
        );
        if let Some(drawing) = drawing.as_deref_mut() {
            drawing.update(
                output.as_ref().and_then(|output| output.drawing.clone()),
                packet_render::session(&gameplay),
            );
            if let Some(scene) = render_scene.as_deref_mut() {
                packet_render::publish_alone(scene, drawing);
            }
        }
        let source = output.as_ref().map_or(0, |output| output.camera_sequence);
        let effects = output.and_then(|output| output.camera);
        if let Some(camera) = camera.as_deref_mut() {
            camera.set_preserve_teleport_rotation(
                effects
                    .as_ref()
                    .is_some_and(|effects| effects.preserve_teleport_rotation),
            );
            camera.set_view_tilt(
                effects
                    .as_ref()
                    .and_then(|effects| effects.view_tilt)
                    .map(native_camera::view_tilt),
            );
            native_camera.apply(
                camera,
                gameplay.view.as_deref_mut(),
                user_freelook,
                effects.as_ref(),
                source,
                now,
            );
        }
        let delta = effects
            .as_ref()
            .and_then(|effects| effects.rotation)
            .map(|delta| mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            });
        gameplay.apply_native_controls(effects.as_ref(), delta, &ui);
        clear_touch_buttons(&mut presentation);
        gameplay.apply_movement_intents(None, false);
        gameplay.apply_control_modes(None);
        return;
    };
    if extension.suspended {
        time_override.0 = None;
        action_filter::inactive(
            &mut action_filter,
            gameplay.input.as_deref_mut(),
            time.elapsed().as_micros(),
        );
        *action_scope = None;
        gameplay.apply_native_input(None, &ui);
        gameplay.apply_movement_intents(None, false);
        gameplay.apply_control_modes(None);
        if let Some(drawing) = drawing.as_deref_mut() {
            drawing.update(None, packet_render::session(&gameplay));
            if let Some(scene) = render_scene.as_deref_mut() {
                packet_render::publish_alone(scene, drawing);
            }
        }
        if let Some(packets) = packets.as_ref() {
            packets.native_frame(None);
        }
        if let Some(camera) = camera.as_deref_mut() {
            camera.set_preserve_teleport_rotation(false);
            camera.set_view_tilt(None);
            native_camera.apply(
                camera,
                gameplay.view.as_deref_mut(),
                user_freelook,
                None,
                0,
                now,
            );
        }
        clear_touch_buttons(&mut presentation);
        return;
    }
    // Taps completed this frame reach their owners before frame callbacks read them.
    for (owner, id) in presentation.take_mod_touch_taps() {
        extension.hosts.queue_touch_press(owner, &id);
    }
    drawing_policy::bind(&mut extension.hosts, &gameplay);
    anchors::reconcile(&mut extension.hosts, &gameplay);
    // Reconcile screen departures before frame callbacks retry deferred edits.
    let cursor = windows
        .single()
        .ok()
        .and_then(|(window, _)| window.cursor_position())
        .map(|point| point.to_array());
    screens::drive(
        &mut extension,
        &player_runtime,
        &ui,
        &mut presentation,
        menu.as_deref(),
        cursor,
        focused && !menu_open,
        &mut screen_input,
    );
    let pressed = keybind_allowed(focused, absorbed) && keys.just_pressed(DEMO_KEY);
    let mut controls = std::mem::replace(&mut extension.controls, mod_host::empty_controls());
    controls.keys_admitted = bindings::admitted_keys(
        gameplay.input.as_deref().and_then(|input| input.snapshot()),
        captured && !absorbed,
    );
    let chat_events = std::mem::take(&mut extension.chat_events);
    let previous_cues = cues.as_ref().map_or_else(Vec::new, |feed| feed.0.clone());
    let registrations: Vec<_> = extension
        .metadata
        .iter()
        .map(|metadata| metadata.registration.clone())
        .collect();
    let player_state = gameplay.player_state(
        (0..extension.hosts.host_count()).any(|index| {
            let host = extension.hosts.host(index);
            host.is_active() && host.grants().player_state
        }),
        &player_runtime,
        &ui,
    );
    extension.hosts.set_food_catalog(ui.food_catalog());
    let world_observation = gameplay.world_observation(&ui);
    let actor_reads = (0..extension.hosts.host_count()).any(|index| {
        let host = extension.hosts.host(index);
        host.is_active() && (host.grants().players || host.grants().entities)
    });
    let modules = module_observer.capture(
        &gameplay,
        &ui,
        world_observation.as_ref(),
        &player_runtime,
        Some(&interaction),
        actor_reads,
    );
    let chat = if world_observation.is_some() {
        presentation_facts::chat_snapshot(&ui)
    } else {
        Vec::new()
    };
    let fps = frame_rate.sample(std::time::Instant::now());
    let sidebar = world_observation
        .as_ref()
        .and_then(|_| presentation.sidebar_snapshot(&ui))
        .map(|(title, rows)| mod_host::SidebarSnapshot {
            title,
            rows: rows
                .into_iter()
                .map(|(label, score)| mod_host::SidebarRow { label, score })
                .collect(),
        });
    terrain.refresh(&mut extension.hosts, &gameplay, world_observation.as_ref());
    let (hud_viewport, hud_elements) = presentation.hud_elements();
    let label_metrics = presentation.label_metrics();
    extension
        .hosts
        .observe_hud(hud_viewport, hud_elements, label_metrics);
    let panel_layout = presentation.mod_ui().mod_panel_layout();
    for index in 0..extension.hosts.host_count() {
        let owner = extension.panel_ui_owner(index);
        extension.hosts.host_mut(index).observe_panel_layout(
            panel_layout
                .filter(|(laid_out, _)| *laid_out == owner)
                .map(|(_, layout)| layout),
        );
    }
    let panel_owner = extension.hosts.panel_owner();
    let panel_was_recovering = panel_owner < extension.hosts.host_count()
        && !packets
            .as_ref()
            .is_some_and(|packets| packets.menu_owns_panel())
        && extension.hosts.host(panel_owner).is_recovering();
    let recovering_screen = extension
        .hosts
        .screen_owner()
        .filter(|&owner| extension.hosts.host(owner).is_recovering());
    let mut callback_failures = Vec::new();
    if let Some(input_settings) = input_settings.as_ref() {
        let bindings = bindings.sample(input_settings.controls_update().1);
        for index in 0..extension.hosts.host_count() {
            if let Err(error) = extension
                .hosts
                .host_mut(index)
                .observe_input_bindings(std::sync::Arc::clone(&bindings))
            {
                warn!(%error, "mod input bindings rejected");
            }
        }
    }
    multi::run_frame_callbacks(
        &mut extension.hosts,
        multi::FrameInput {
            pressed,
            controls: &controls,
            previous_cues: &previous_cues,
            player_state: player_state.as_ref(),
            world_observation: world_observation.as_ref(),
            fps,
            modules: Some(&modules),
            sidebar: sidebar.as_ref(),
            chat: &chat,
            chat_events: &chat_events,
            gameplay_facts: gameplay.facts(
                captured && !absorbed,
                cameras.single().ok().map(|(camera, _)| camera),
                world_observation
                    .as_ref()
                    .and(relayed.as_deref())
                    .and_then(|relayed| relayed.position),
                world_observation
                    .as_ref()
                    .and(damage_shake.as_deref())
                    .and_then(|shake| shake.0),
            ),
        },
        |grants| {
            let snapshot = gameplay.snapshot(captured && !absorbed, grants);
            let mobs = gameplay.mobs(snapshot.as_ref(), grants);
            let movement = gameplay.movement_snapshot(captured && !absorbed, grants, &ui);
            (snapshot, mobs, movement)
        },
        |index, error| {
            callback_failures.push((index, error));
        },
    );
    if panel_was_recovering && !extension.hosts.host(panel_owner).is_recovering() {
        presentation.mod_ui_mut().cancel_mod_panel_edit();
        presentation.mod_ui_mut().cancel_mod_panel_pointer_input();
    }
    if recovering_screen.is_some_and(|owner| !extension.hosts.host(owner).is_recovering()) {
        presentation.mod_ui_mut().reset_mod_screens();
    }
    let mut callback_failed = !callback_failures.is_empty();
    for (index, error) in callback_failures {
        if !extension.hosts.host(index).is_active()
            && let Some(registered) = &registrations[index]
            && let Some(watcher) = watcher.as_ref()
        {
            watcher.quarantine(&registered.entry, error.clone());
        }
        warn!("Cinnabar extension callback failed: {error}");
    }
    // A tapped mod that opened its panel shows it, as its toggle key would.
    extension.hosts.select_panels_opened_by_taps();
    for index in 0..extension.hosts.host_count() {
        for message in extension.hosts.host_mut(index).take_local_messages() {
            ui.push_local_chat_line(
                message.into(),
                u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX),
            );
        }
    }

    if let Some(watcher) = watcher.as_ref() {
        watcher.remember_settings(Some(&extension));
    }
    let scope = world_observation
        .as_ref()
        .map(|world| (world.session, world.dimension));
    // Controller and touch gestures reach gameplay without cursor capture, so they are
    // filtered under the same source gates.
    let source_admitted = gameplay
        .input
        .as_ref()
        .and_then(|input| input.snapshot())
        .is_some_and(|input| {
            crate::semantic_controls::physical::admits_input_mode(
                input.input_mode,
                focused,
                captured,
            )
        });
    if *action_scope != scope || !source_admitted || absorbed {
        action_filter.reset();
    }
    let enabled = source_admitted && !absorbed && scope.is_some();
    let attack_requested = extension
        .hosts
        .iter_mut()
        .any(|host| host.attack_pulse_requested());
    let use_requested = extension
        .hosts
        .iter_mut()
        .any(|host| host.use_pulse_requested());
    let ([attack_pulse, use_pulse], filter_error) = action_filter::apply(
        &mut action_filter,
        gameplay.input.as_deref_mut(),
        &mut extension.hosts,
        enabled,
        time.elapsed().as_micros(),
        [attack_requested, use_requested],
    );
    interaction.attack_pulse = attack_pulse;
    if let Some(policy) = item_use.as_deref_mut() {
        policy.pulse = use_pulse;
    }
    if let Some((index, error)) = filter_error {
        callback_failed = true;
        if let Some(registered) = &registrations[index]
            && let Some(watcher) = watcher.as_ref()
        {
            watcher.quarantine(&registered.entry, error.clone());
        }
        warn!("Cinnabar action filter failed: {error}");
    }
    let merged = multi::collect_frame_outputs(&mut extension.hosts);
    if callback_failed || merged.jump_cancel {
        gameplay.synchronize_jump_scope(false, &ui);
    }
    let movement_allowed = (0..extension.hosts.host_count()).any(|index| {
        extension.hosts.host(index).is_active() && extension.hosts.host(index).grants().movement
    });
    gameplay.synchronize_jump_scope(movement_allowed && captured && !absorbed, &ui);
    gameplay.pulse_jump(merged.jump_pulse && !merged.jump_cancel, &ui);
    interaction.attack_reach = merged.attack_reach;
    let regular_camera = (0..extension.hosts.host_count()).any(|index| {
        let host = extension.hosts.host(index);
        host.is_active() && host.has_camera_output()
    });
    let native_output = exchange_native_frame(
        packets.as_deref(),
        &mut native_camera,
        &gameplay,
        captured && !absorbed && !regular_camera,
        &ui,
        (time.delta_secs(), now),
    );
    if let Some(drawing) = drawing.as_deref_mut() {
        drawing.update(
            native_output
                .as_ref()
                .and_then(|output| output.drawing.clone()),
            packet_render::session(&gameplay),
        );
    }
    render::publish(render_scene, &mut extension, drawing.as_deref());
    let native_source = native_output
        .as_ref()
        .map_or(0, |output| output.camera_sequence);
    let native_effects = native_output.and_then(|output| output.camera);
    let delta = merged.delta.or_else(|| {
        native_effects
            .as_ref()
            .and_then(|effects| effects.rotation)
            .map(|delta| mod_host::CameraDelta {
                yaw: delta.yaw,
                pitch: delta.pitch,
            })
    });
    gameplay.apply_native_controls(
        native_effects.as_ref().filter(|_| !regular_camera),
        delta,
        &ui,
    );
    gameplay.apply_movement_intents(merged.movement_intents, captured && !absorbed);
    gameplay.apply_control_modes(merged.control_modes);
    send_commands(network.as_deref(), ui.session_id(), merged.commands);
    if let Some(mut cues) = cues {
        cues.0 = merged.cues;
    }
    if let Some(mut camera) = camera {
        camera.set_rig(merged.rig.map(camera_rig));
        let native_effects = native_effects.as_ref().filter(|_| !regular_camera);
        camera.set_preserve_teleport_rotation(
            merged.preserve_teleport_rotation
                || native_effects.is_some_and(|effects| effects.preserve_teleport_rotation),
        );
        let [fov_scale, look_scale] = merged.view_scale.unwrap_or([1.0, 1.0]);
        camera.set_view_scale(fov_scale, look_scale);
        camera.set_view_tilt(
            merged
                .view_tilt
                .or_else(|| native_effects.and_then(|effects| effects.view_tilt))
                .map(native_camera::view_tilt),
        );
        native_camera.apply(
            &mut camera,
            gameplay.view.as_deref_mut(),
            user_freelook,
            native_effects,
            native_source,
            now,
        );
    }
    time_override.0 = merged.time_override;
    if let Some(weather) = weather.as_deref_mut() {
        weather.0 = merged.weather_override;
    }
    *action_scope = world_observation
        .as_ref()
        .map(|world| (world.session, world.dimension));
    if let Err(error) = presentation
        .mod_ui_mut()
        .set_mod_label(extension.hosts.merged_label())
    {
        warn!("Cinnabar extension HUD rejected: {error}");
    }
    let hidden = extension.hosts.hidden_hud_elements();
    let mod_ui = presentation.mod_ui_mut();
    mod_ui.set_hud_surfaces(extension.hud_surfaces());
    mod_ui.set_hidden_hud_elements(hidden);
    let mod_presentation = extension.hosts.merged_presentation();
    if let Some(style) = actor_style.as_mut()
        && !std::sync::Arc::ptr_eq(&style.presentation, &mod_presentation)
    {
        style.presentation = std::sync::Arc::clone(&mod_presentation);
    }
    presentation.set_mod_presentation(mod_presentation);
    if !packets
        .as_ref()
        .is_some_and(|packets| packets.menu_owns_panel())
    {
        let owner = extension.hosts.panel_owner();
        presentation.set_mod_menu_branding(extension.menu_branding());
        if let Err(error) = presentation.set_mod_panel_font(extension.metadata[owner].font.clone())
        {
            warn!("Cinnabar extension panel font rejected: {error}");
        }
        presentation
            .mod_ui_mut()
            .set_mod_panel_actor_authority(actor_authority(extension.hosts.host(owner).grants()));
        if let Err(error) = presentation.mod_ui_mut().set_mod_panel(
            extension.panel_ui_owner(owner),
            extension.hosts.host(owner).panel(),
        ) {
            warn!("Cinnabar extension panel rejected: {error}");
            extension.hosts.host_mut(owner).set_panel_open(false);
        }
        presentation
            .mod_ui_mut()
            .set_mod_panel_open(extension.hosts.host(owner).panel_open());
    }
    let hosts = &extension.hosts;
    presentation.set_mod_touch_buttons(
        hosts
            .touch_buttons()
            .into_iter()
            .map(|(index, button)| (hosts.host(index).touch_owner(), button)),
    );
    if let Some(owner) = extension.hosts.screen_owner() {
        screens::publish(extension.hosts.host(owner), &mut presentation);
    } else {
        presentation.set_mod_screens(None);
    }
}

/// Removes mod touch buttons and drops their taps while no mod can receive them.
fn clear_touch_buttons(presentation: &mut UiPresentationResource) {
    presentation.take_mod_touch_taps();
    presentation.set_mod_touch_buttons(std::iter::empty());
}

/// Builds this frame's packet observation with its captured look and real time, hands it to
/// the packet mod and records it for rebasing that callback's view. `time` is the real frame
/// duration and elapsed seconds.
fn exchange_native_frame(
    packets: Option<&packets::PacketMods>,
    state: &mut native_camera::NativeCamera,
    gameplay: &gameplay::GameplayContext,
    allowed: bool,
    ui: &client_ui::ui_runtime::UiRuntime,
    (seconds, now): (f32, f64),
) -> Option<client_extensions::modding::packet_relay::FrameOutput> {
    let packets = packets.filter(|packets| packets.native_frame_available())?;
    let frame = state.prepare(
        gameplay.view.as_deref(),
        seconds,
        gameplay.native_frame(allowed, ui),
    );
    let sent = frame.is_some();
    let output = packets.native_frame(frame);
    state.exchanged(
        output
            .as_ref()
            .filter(|_| sent)
            .map(|output| output.sequence),
        now,
    );
    output
}
