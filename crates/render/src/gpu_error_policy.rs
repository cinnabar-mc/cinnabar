//! Terminal GPU failures passed to the application for its restart error screen.

use bevy::render::error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy};

/// Stops rendering so the app can report the GPU failure before requesting an exit.
/// Install after the render plugin so this handler replaces the default.
pub fn install_error_policy(app: &mut bevy::app::App) {
    app.init_resource::<GpuFailure>()
        .insert_resource(RenderErrorHandler(error_policy));
}

/// The first terminal GPU failure; the app stops and shows the driver's report.
#[derive(bevy::prelude::Resource, Default)]
pub struct GpuFailure(Option<String>);

impl GpuFailure {
    /// The driver's description once rendering cannot safely continue.
    pub fn message(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Keeps the first failure; Bevy repeats a stopped renderer's error every frame.
    pub fn record(&mut self, description: &str) {
        self.0.get_or_insert_with(|| description.to_owned());
    }
}

/// Stops unsafe rendering while the main world reports the first terminal GPU error.
fn error_policy(
    error: &RenderError,
    main_world: &mut bevy::ecs::world::World,
    _render_world: &mut bevy::ecs::world::World,
) -> RenderErrorPolicy {
    let failure = main_world.get_resource_or_init::<GpuFailure>();
    if failure.message().is_none() {
        // The source can contain validation context absent from the short description.
        bevy::log::error!(target: "cinnabar::render", ?error, "Renderer stopped after a GPU error");
    }
    let description = if error.description.is_empty() {
        match error.ty {
            ErrorType::OutOfMemory => "uncaptured GPU out-of-memory error",
            ErrorType::DeviceLost => "GPU device lost",
            ErrorType::Validation => "GPU validation error",
            ErrorType::Internal => "internal GPU error",
        }
    } else {
        &error.description
    };
    main_world.resource_mut::<GpuFailure>().record(description);
    // Extraction follows the main update. An immediate exit here would skip cause capture.
    RenderErrorPolicy::StopRendering
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        app::AppExit,
        ecs::{message::Messages, world::World},
    };

    /// Both terminal GPU errors stop without a bare exit and keep the first driver report.
    #[test]
    fn terminal_gpu_errors_stop_rendering_and_record_the_first_reason() {
        for ty in [ErrorType::DeviceLost, ErrorType::OutOfMemory] {
            let (mut main_world, mut render_world) = (World::new(), World::new());
            main_world.init_resource::<Messages<AppExit>>();
            for description in ["first driver report", "repeated"] {
                let error = RenderError {
                    ty,
                    description: description.into(),
                    source: None,
                };
                let policy = error_policy(&error, &mut main_world, &mut render_world);
                assert!(matches!(policy, RenderErrorPolicy::StopRendering), "{ty:?}");
            }
            assert!(main_world.resource::<Messages<AppExit>>().is_empty());
            assert_eq!(
                main_world.resource::<GpuFailure>().message(),
                Some("first driver report")
            );
        }
    }

    /// Empty driver messages still identify the failure category.
    #[test]
    fn empty_driver_reports_have_a_specific_fallback() {
        for (ty, expected) in [
            (ErrorType::DeviceLost, "GPU device lost"),
            (ErrorType::OutOfMemory, "uncaptured GPU out-of-memory error"),
            (ErrorType::Validation, "GPU validation error"),
            (ErrorType::Internal, "internal GPU error"),
        ] {
            let (mut main, mut render) = (World::new(), World::new());
            main.init_resource::<Messages<AppExit>>();
            error_policy(
                &RenderError {
                    ty,
                    description: String::new(),
                    source: None,
                },
                &mut main,
                &mut render,
            );
            assert_eq!(main.resource::<GpuFailure>().message(), Some(expected));
        }
    }

    /// Validation and internal failures wait for the app to capture their cause before exiting.
    #[test]
    fn other_errors_stop_rendering_without_a_premature_exit() {
        for ty in [ErrorType::Validation, ErrorType::Internal] {
            let (mut main_world, mut render_world) = (World::new(), World::new());
            main_world.init_resource::<Messages<AppExit>>();
            let error = RenderError {
                ty,
                description: "driver report".into(),
                source: None,
            };
            let policy = error_policy(&error, &mut main_world, &mut render_world);
            assert!(matches!(policy, RenderErrorPolicy::StopRendering));
            assert!(main_world.resource::<Messages<AppExit>>().is_empty());
            assert_eq!(
                main_world.resource::<GpuFailure>().message(),
                Some("driver report")
            );
        }
    }
}
