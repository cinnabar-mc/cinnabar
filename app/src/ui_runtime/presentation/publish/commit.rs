//! Pure UI rasterization, layout and publication after input enqueue.
use super::*;
use bevy::prelude::{DetectChanges, DetectChangesMut};
use std::sync::Arc;

/// Publishes the UI captured during preparation, repeating the last frame when nothing it was
/// built from changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_ui_runtime(
    player_runtime: Res<client_state::PlayerRuntime>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut prepared: ResMut<PreparedUiPublication>,
    mut presentation: ResMut<UiPresentationResource>,
    mut scene: ResMut<render_scene::UiRenderSceneResource>,
    stats: Res<render_scene::UiRenderStatsResource>,
    hand_rig: Res<render_scene::HandRigScene>,
    nametag_scene: Option<ResMut<render_scene::NametagSceneResource>>,
    mut hand: crate::presentation::viewmodel::ViewmodelPublish,
    profiler: Option<Res<render_scene::RuntimeStageProfiler>>,
    mut item_glint: Option<ResMut<render_scene::UiItemGlintTexture>>,
    mut inputs_changed: Local<InputsChanged>,
) {
    // The frame's revisioned inputs: change ticks of every resource a build reads. This system's
    // own writes never count, and changes accumulate until a build has seen them.
    inputs_changed.0 |=
        runtime.is_changed() || player_runtime.is_changed() || presentation.is_changed();
    let Some(prepared) = prepared.0.take() else {
        return;
    };
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::UiPublication));
    // Publication presents the captured inventory by swapping it into the runtime and back, so
    // it leaves the runtime as it found it: the runtime's change tick stays a revision of what
    // other systems wrote. Building writes only the presentation's frame caches, which no other
    // system reads, so its change tick likewise stays a revision of the inputs others wrote.
    let presentation = presentation.bypass_change_detection();
    let built = match client_ui::ui_runtime::presentation::render_prepared_ui(
        &player_runtime,
        runtime.bypass_change_detection(),
        presentation,
        prepared,
        !inputs_changed.0,
    ) {
        Ok(built) => built,
        Err(error) => {
            hand.clear();
            presentation.record_frame_failure(&error);
            return;
        }
    };
    inputs_changed.0 = false;
    if let Some(mut scene) = nametag_scene {
        // Writing only a changed scene keeps the resource unchanged, so render extraction
        // neither clones nor re-uploads identical labels.
        let next = presentation.nametag_scene();
        if scene.0 != next {
            scene.0 = next;
        }
    }
    let Some(input) = built else {
        // The published frame repeats; the hand still binds this frame's capture to it.
        if !hand_rig.is_active()
            && let Some(input) = scene.input.as_deref()
        {
            hand.bind_cpu_fallback(
                input,
                presentation.cpu_empty_hand_fallback(),
                presentation.hud_frame().held_item_icon,
            );
        }
        return;
    };
    if !hand_rig.is_active() {
        hand.bind_cpu_fallback(
            &input,
            presentation.cpu_empty_hand_fallback(),
            presentation.hud_frame().held_item_icon,
        );
    }
    // Republishing the accepted frame leaves the resource unchanged, so render extraction skips
    // its clone; any accepted or rejected publication changes the revision or input.
    let published = (scene.revision, scene.input.as_ref().map(Arc::as_ptr));
    let result = scene.bypass_change_detection().publish(input, &stats);
    if published != (scene.revision, scene.input.as_ref().map(Arc::as_ptr)) {
        scene.set_changed();
    }
    if let Err(error) = result {
        hand.clear();
        presentation.forget_built_frame();
        presentation.record_frame_failure(&UiPresentationError::Render(error));
    } else if let Some(item_glint) = item_glint.as_mut() {
        let next = presentation.item_glint_texture();
        let same = match (item_glint.0.as_ref(), next) {
            (Some(current), Some(next)) => Arc::ptr_eq(current, next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            item_glint.0 = next.cloned();
        }
    }
}

/// Whether a revisioned input of the UI frame changed since the last build saw them.
#[derive(Default)]
pub(crate) struct InputsChanged(bool);

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::prelude::{DetectChanges, World};

    /// Supplies only CPU presentation owners, without a window or network connection.
    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )));
        world.insert_resource(UiRuntimeResource(UiRuntime::new(1)));
        let mut presentation =
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
        presentation.set_menu_view(Some(launcher::menu::MenuView::new(true, "Fixture".into())));
        world.insert_resource(UiPresentationResource(presentation));
        world.init_resource::<PreparedUiPublication>();
        world.init_resource::<render_scene::UiRenderSceneResource>();
        world.init_resource::<render_scene::UiRenderStatsResource>();
        world.init_resource::<ClientWorld>();
        world.init_resource::<crate::runtime::shutdown::FatalError>();
        world.init_resource::<render_scene::HandRigScene>();
        world
    }

    /// Publishes a captured frame through the same system that drives a connected session.
    fn publish(world: &mut World, size: [u32; 2]) {
        let inventory = world
            .resource::<UiRuntimeResource>()
            .capture_presentation_inventory(world.resource::<client_state::PlayerRuntime>());
        world.resource_mut::<PreparedUiPublication>().0 = Some(PendingUiPublication {
            inventory,
            preview: PreviewCapture {
                skin: None,
                ready: false,
                pose: Default::default(),
                shown: false,
                hands: false,
            },
            item_icons: (None, None),
            now_millis: 0,
            physical_size: size,
            dpi_scale: DpiScale::new(1.0).unwrap(),
        });
        world.run_system_once(publish_ui_runtime).unwrap();
    }

    /// The session's recorded fatal error, if publication ended it.
    fn fatal_error(world: &World) -> &Option<String> {
        &world.resource::<crate::runtime::shutdown::FatalError>().0
    }

    /// A warm, unchanged frame republishes nothing, so render extraction does not clone the scene.
    #[test]
    fn unchanged_frame_leaves_the_published_scene_unchanged() {
        let mut world = world();
        let published = |world: &World| {
            let scene = world.resource_ref::<render_scene::UiRenderSceneResource>();
            (scene.last_changed(), scene.revision, scene.input.is_some())
        };
        publish(&mut world, [1280, 720]);
        let first = published(&world);
        assert!(first.2);
        world.increment_change_tick();
        publish(&mut world, [1280, 720]);
        assert_eq!(published(&world), first);
        world.increment_change_tick();
        publish(&mut world, [1024, 600]);
        let resized = published(&world);
        assert!(resized.0 != first.0 && resized.1 > first.1);
    }

    #[test]
    fn rejected_render_publication_does_not_end_the_session() {
        let mut world = world();
        world
            .resource_mut::<render_scene::UiRenderSceneResource>()
            .revision = u64::MAX;
        publish(&mut world, [1280, 720]);
        assert!(fatal_error(&world).is_none(), "{:?}", fatal_error(&world));
        assert!(
            world
                .resource::<render_scene::UiRenderSceneResource>()
                .input
                .is_none()
        );
    }

    #[test]
    fn overflowing_text_publishes_without_ending_the_session() {
        let mut world = world();
        let mut view = launcher::menu::MenuView::new(true, "Fixture".into());
        view.screen = launcher::menu::MenuScreen::Settings;
        world
            .resource_mut::<UiPresentationResource>()
            .set_menu_view(Some(view));
        publish(&mut world, [254, 124]);
        assert!(fatal_error(&world).is_none(), "{:?}", fatal_error(&world));
        assert!(
            world
                .resource::<render_scene::UiRenderSceneResource>()
                .input
                .is_some()
        );
        publish(&mut world, [1280, 720]);
        assert!(fatal_error(&world).is_none(), "{:?}", fatal_error(&world));
    }

    #[test]
    fn rejected_ui_frame_does_not_end_the_session_and_next_frame_publishes() {
        for size in [[0, 0], [1, 1]] {
            let mut world = world();
            publish(&mut world, size);
            assert!(fatal_error(&world).is_none(), "{:?}", fatal_error(&world));
            assert_eq!(
                world
                    .resource::<UiPresentationResource>()
                    .rejected_frame_count(),
                1
            );
            publish(&mut world, [1280, 720]);
            assert!(fatal_error(&world).is_none(), "{:?}", fatal_error(&world));
            assert!(
                world
                    .resource::<render_scene::UiRenderSceneResource>()
                    .input
                    .is_some()
            );
        }
    }
}
