//! Unmaps closing Wayland surfaces without destroying handles retained by the renderer.

use bevy::{
    ecs::schedule::MainThreadExecutor,
    prelude::{Res, ResMut},
    render::pipelined_rendering::RenderAppChannels,
    tasks::ComputeTaskPool,
    window::RawHandleWrapper,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::{
    Connection, Proxy,
    backend::{Backend, ObjectId},
    protocol::wl_surface::{Request, WlSurface},
};

/// Reports whether this live window uses a Wayland surface.
pub(super) fn is_wayland(handle: &RawHandleWrapper) -> bool {
    matches!(handle.get_window_handle(), RawWindowHandle::Wayland(_))
}

/// Finishes the in-flight frame and prevents later presents from remapping an unmapped surface.
pub(super) fn stop_rendering(
    channels: Option<ResMut<RenderAppChannels>>,
    executor: Option<Res<MainThreadExecutor>>,
) {
    let Some(mut channels) = channels else {
        return;
    };
    let executor = executor.expect("Pipelined rendering requires its main-thread executor");
    let render_app = ComputeTaskPool::get()
        .scope_with_executor(true, Some(&executor.0), |scope| {
            scope.spawn(async { channels.recv().await });
        })
        .pop()
        .flatten();
    if let Some(mut render_app) = render_app {
        render_app.update_schedule = None;
        channels.send_blocking(render_app);
    }
}

/// Detaches the surface content and flushes it before the event loop enters blocking teardown.
pub(super) fn unmap(handle: &RawHandleWrapper) -> anyhow::Result<()> {
    let (RawWindowHandle::Wayland(window), RawDisplayHandle::Wayland(display)) =
        (handle.get_window_handle(), handle.get_display_handle())
    else {
        return Ok(());
    };
    // SAFETY: The wrapper keeps this display and surface alive throughout the call. The borrowed
    // backend neither owns nor dispatches winit's connection, and no rendering resumes after unmap.
    let (backend, id) = unsafe {
        (
            Backend::from_foreign_display(display.display.as_ptr().cast()),
            ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr().cast())?,
        )
    };
    let connection = Connection::from_backend(backend);
    let surface = WlSurface::from_id(&connection, id)?;
    surface.send_request(Request::Attach {
        buffer: None,
        x: 0,
        y: 0,
    })?;
    surface.send_request(Request::Commit)?;
    connection.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests;
