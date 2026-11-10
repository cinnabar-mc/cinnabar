//! UIKit hover positions for the unlocked mouse pointer.

use std::cell::RefCell;

use bevy::{ecs::system::NonSendMarker, prelude::*, window::PrimaryWindow, winit::WINIT_WINDOWS};
use objc2::{
    MainThreadMarker, MainThreadOnly, class, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_foundation::NSPoint;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{NativeWindowEvent, queue};

/// UIKit's terminal gesture states, which clear the menu pointer.
const GESTURE_ENDED: isize = 3;
const GESTURE_CANCELLED: isize = 4;

define_class!(
    // SAFETY: NSObject has no subclassing requirements; UIKit invokes the target on main.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "CinnabarHoverTarget"]
    struct HoverTarget;

    impl HoverTarget {
        /// Queues the pointer's position in the view, preserving callback order.
        #[unsafe(method(hoverChanged:))]
        fn hover_changed(&self, recognizer: &AnyObject) {
            // SAFETY: The sender is the UIHoverGestureRecognizer installed below. UIKit
            // supplies its view and returns CGPoint/CGFloat in the view's coordinate space.
            let position = unsafe {
                let state: isize = msg_send![recognizer, state];
                if matches!(state, GESTURE_ENDED | GESTURE_CANCELLED) {
                    None
                } else {
                    let view: Option<Retained<AnyObject>> = msg_send![recognizer, view];
                    let Some(view) = view else { return };
                    let point: NSPoint = msg_send![recognizer, locationInView: &*view];
                    let scale: f64 = msg_send![&*view, contentScaleFactor];
                    Some(Vec2::new((point.x * scale) as f32, (point.y * scale) as f32))
                }
            };
            queue(NativeWindowEvent::Cursor(position));
        }
    }
);

/// Retains the target while its recognizer is attached to a live winit view.
struct Hover {
    view: Retained<AnyObject>,
    recognizer: Retained<AnyObject>,
    _target: Retained<HoverTarget>,
}

thread_local! {
    static HOVER: RefCell<Option<Hover>> = const { RefCell::new(None) };
}

/// Attaches one hover recognizer to the primary view without consuming touch input.
pub(super) fn attach(
    window: Option<Single<Entity, With<PrimaryWindow>>>,
    _main_thread: NonSendMarker,
) {
    let Some(window) = window else { return };
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(*window) else {
            return;
        };
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
            return;
        };
        HOVER.with_borrow_mut(|hover| {
            let view = handle.ui_view.as_ptr().cast::<AnyObject>();
            if hover
                .as_ref()
                .is_some_and(|hover| std::ptr::eq(Retained::as_ptr(&hover.view), view))
            {
                return;
            }
            // SAFETY: The raw handle names winit's live UIView; all access stays on main.
            let Some(view) = (unsafe { Retained::retain(view) }) else {
                return;
            };
            *hover = Some(Hover::new(view, main_thread));
        });
    });
}

impl Hover {
    /// Installs a target retained for the recognizer's entire lifetime.
    fn new(view: Retained<AnyObject>, main_thread: MainThreadMarker) -> Self {
        // SAFETY: NSObject init and UIKit's target/action signatures match these types.
        let (target, recognizer) = unsafe {
            let target: Retained<HoverTarget> =
                msg_send![super(HoverTarget::alloc(main_thread)), init];
            let recognizer: Allocated<AnyObject> =
                msg_send![class!(UIHoverGestureRecognizer), alloc];
            let recognizer: Retained<AnyObject> = msg_send![recognizer,
                initWithTarget: &*target, action: sel!(hoverChanged:)];
            let _: () = msg_send![&*recognizer, setCancelsTouchesInView: false];
            let _: () = msg_send![&*view, addGestureRecognizer: &*recognizer];
            (target, recognizer)
        };
        Self {
            view,
            recognizer,
            _target: target,
        }
    }
}

impl Drop for Hover {
    /// Detaches the recognizer before releasing its target when the view changes.
    fn drop(&mut self) {
        // SAFETY: These retained UIKit objects are accessed only on the main thread.
        unsafe {
            let _: () = msg_send![&*self.view, removeGestureRecognizer: &*self.recognizer];
        }
    }
}
