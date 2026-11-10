//! Samples the platform's settled fullscreen state, which can lag the requested window mode.

use bevy::{
    ecs::system::NonSendMarker,
    prelude::{Entity, Query, ResMut, Resource, With},
    window::PrimaryWindow,
    winit::WINIT_WINDOWS,
};

/// The primary window's platform-confirmed fullscreen state; `None` while unknown or in transition.
#[derive(Default, Resource)]
pub struct NativeFullscreen(pub(super) Option<bool>);

/// Leaves the state untouched while no native window exists, as in headless adapters.
pub(super) fn sample(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut native: ResMut<NativeFullscreen>,
    _main_thread: NonSendMarker,
) {
    let Ok(entity) = windows.single() else {
        return;
    };
    if let Some(state) = WINIT_WINDOWS
        .with_borrow(|windows| windows.get_window(entity).map(|window| settled(window)))
    {
        native.0 = state;
    }
}

/// Wayland reports the compositor's last configure and Windows applies requests synchronously.
/// X11 reports the request before the window manager applies it.
#[cfg(not(target_os = "macos"))]
fn settled(window: &winit::window::Window) -> Option<bool> {
    Some(window.fullscreen().is_some())
}

#[cfg(target_os = "macos")]
use macos::settled;

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        cell::RefCell,
        ptr::NonNull,
        sync::{
            Arc,
            atomic::{AtomicU8, Ordering},
        },
    };

    use block2::RcBlock;
    use objc2::{
        MainThreadMarker,
        rc::Retained,
        runtime::{AnyObject, NSObjectProtocol, ProtocolObject},
    };
    use objc2_app_kit::{
        NSView, NSWindow, NSWindowDidEnterFullScreenNotification,
        NSWindowDidExitFullScreenNotification, NSWindowStyleMask,
        NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    const WINDOWED: u8 = 0;
    const ENTERING: u8 = 1;
    const FULLSCREEN: u8 = 2;
    const EXITING: u8 = 3;

    /// AppKit transition notifications for one native window, removed when it is replaced.
    struct Observer {
        window: Retained<NSWindow>,
        phase: Arc<AtomicU8>,
        tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    }

    thread_local! {
        static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
    }

    /// Reports fullscreen only after AppKit posts that the window finished entering it.
    pub(super) fn settled(window: &winit::window::Window) -> Option<bool> {
        let _main_thread = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit owns this live NSView, and AppKit access stays on the main thread.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let window = view.window()?;
        OBSERVER.with_borrow_mut(|observer| {
            if observer.as_ref().is_none_or(|observer| {
                !std::ptr::eq(
                    Retained::as_ptr(&observer.window),
                    Retained::as_ptr(&window),
                )
            }) {
                *observer = Some(Observer::new(window));
            }
            match observer.as_ref()?.phase.load(Ordering::Acquire) {
                WINDOWED => Some(false),
                FULLSCREEN => Some(true),
                _ => None,
            }
        })
    }

    impl Observer {
        /// Starts from the window's current style so an already-fullscreen window reads settled.
        fn new(window: Retained<NSWindow>) -> Self {
            let initial = if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
                FULLSCREEN
            } else {
                WINDOWED
            };
            let phase = Arc::new(AtomicU8::new(initial));
            let center = NSNotificationCenter::defaultCenter();
            // SAFETY: These are immutable AppKit name constants.
            let names = unsafe {
                [
                    (NSWindowWillEnterFullScreenNotification, ENTERING),
                    (NSWindowDidEnterFullScreenNotification, FULLSCREEN),
                    (NSWindowWillExitFullScreenNotification, EXITING),
                    (NSWindowDidExitFullScreenNotification, WINDOWED),
                ]
            };
            let object: &AnyObject = &window;
            let tokens = names
                .into_iter()
                .map(|(name, next)| {
                    let phase = Arc::clone(&phase);
                    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                        phase.store(next, Ordering::Release);
                    });
                    // SAFETY: The sender is a live NSWindow, the block only stores a shared atomic,
                    // and without a queue AppKit runs it on the posting main thread.
                    unsafe {
                        center.addObserverForName_object_queue_usingBlock(
                            Some(name),
                            Some(object),
                            None,
                            &block,
                        )
                    }
                })
                .collect();
            Self {
                window,
                phase,
                tokens,
            }
        }
    }

    impl Drop for Observer {
        /// Unregisters every block so a replaced window cannot update the new window's phase.
        fn drop(&mut self) {
            let center = NSNotificationCenter::defaultCenter();
            for token in &self.tokens {
                // SAFETY: Each token was returned by this center's block registration.
                unsafe { center.removeObserver(AsRef::<AnyObject>::as_ref(&**token)) };
            }
        }
    }
}
