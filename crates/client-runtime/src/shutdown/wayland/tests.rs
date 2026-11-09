use super::*;
use crate::shutdown::{ShutdownWatchdog, configure_shutdown};
use bevy::{
    app::{AppExit, OnAppExitSystems, SubApp},
    ecs::schedule::{Schedule, ScheduleLabel},
    prelude::{App, IntoScheduleConfigs, Last, MinimalPlugins},
    render::{Render, RenderApp, pipelined_rendering::PipelinedRenderingPlugin},
    window::{Window, WindowWrapper},
};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WaylandDisplayHandle,
    WaylandWindowHandle, WindowHandle,
};
use std::{
    io::{ErrorKind, Read},
    os::unix::net::UnixStream,
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use wayland_client::{
    delegate_noop,
    protocol::{wl_compositor::WlCompositor, wl_registry::WlRegistry},
};

struct TestState;
delegate_noop!(TestState: ignore WlRegistry);
delegate_noop!(TestState: ignore WlCompositor);
delegate_noop!(TestState: ignore WlSurface);

/// Owns real Wayland client objects backed by an in-memory protocol socket.
struct TestWindow {
    connection: Connection,
    surface: WlSurface,
}

impl HasDisplayHandle for TestWindow {
    /// Borrows the display kept alive by this test window.
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = WaylandDisplayHandle::new(
            NonNull::new(self.connection.backend().display_ptr().cast()).unwrap(),
        );
        // SAFETY: This connection owns the display for the borrowed handle's lifetime.
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(handle)) })
    }
}

impl HasWindowHandle for TestWindow {
    /// Borrows the surface kept alive by this test window and its connection.
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle =
            WaylandWindowHandle::new(NonNull::new(self.surface.id().as_ptr().cast()).unwrap());
        // SAFETY: This window owns the surface for the borrowed handle's lifetime.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Wayland(handle)) })
    }
}

/// Drains requests already flushed to the socket without waiting for an event loop.
fn read_requests(socket: &mut UnixStream) -> Vec<u32> {
    let mut bytes = Vec::new();
    let error = socket.read_to_end(&mut bytes).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
    bytes
        .as_chunks::<{ size_of::<u32>() }>()
        .0
        .iter()
        .map(|word| u32::from_ne_bytes(*word))
        .collect()
}

/// Creates a live surface without needing a compositor or a visible desktop.
fn test_window() -> (WindowWrapper<TestWindow>, UnixStream) {
    let (client, mut server) = UnixStream::pair().unwrap();
    server.set_nonblocking(true).unwrap();
    let connection = Connection::from_socket(client).unwrap();
    let queue = connection.new_event_queue::<TestState>();
    let qh = queue.handle();
    let registry = connection.display().get_registry(&qh, ());
    let compositor = registry.bind::<WlCompositor, _, _>(1, 1, &qh, ());
    let surface = compositor.create_surface(&qh, ());
    connection.flush().unwrap();
    read_requests(&mut server);
    (
        WindowWrapper::new(TestWindow {
            connection,
            surface,
        }),
        server,
    )
}

#[test]
fn exit_unmaps_wayland_before_teardown_and_stops_late_presents() {
    let (window, mut socket) = test_window();
    let surface_id = window.surface.id().protocol_id();
    let handle = RawHandleWrapper::new(&window).unwrap();
    let requests_at_cleanup = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requests_at_cleanup);
    let presents = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&presents);
    let mut render_app = SubApp::new();
    render_app.update_schedule = Some(Render.intern());
    render_app
        .add_schedule(Schedule::new(Render))
        .add_systems(Render, move || {
            counted.fetch_add(1, Ordering::SeqCst);
        });
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<AppExit>()
        .insert_resource(ShutdownWatchdog::new(Duration::from_secs(60), |_| {}));
    app.insert_sub_app(RenderApp, render_app);
    app.add_plugins(PipelinedRenderingPlugin);
    configure_shutdown(&mut app);
    app.add_systems(
        Last,
        (move || {
            observed.lock().unwrap().extend(read_requests(&mut socket));
        })
        .in_set(OnAppExitSystems),
    );
    app.world_mut().spawn((Window::default(), handle));
    app.finish();
    app.cleanup();
    app.update();
    assert!(requests_at_cleanup.lock().unwrap().is_empty());
    app.world_mut().write_message(AppExit::Success);
    app.update();

    let words = requests_at_cleanup.lock().unwrap().clone();
    let requests = WlSurface::interface().requests;
    let attach = requests
        .iter()
        .position(|request| request.name == "attach")
        .unwrap() as u32;
    let commit = requests
        .iter()
        .position(|request| request.name == "commit")
        .unwrap() as u32;
    assert_eq!(
        words,
        [
            surface_id,
            (20 << 16) | attach,
            0,
            0,
            0,
            surface_id,
            (8 << 16) | commit
        ]
    );
    // Returning the renderer again must not present another buffer after the null commit.
    app.update();
    drop(app);
    assert_eq!(presents.load(Ordering::SeqCst), 1);
}
