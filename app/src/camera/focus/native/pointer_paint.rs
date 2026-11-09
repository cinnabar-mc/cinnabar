//! Runs a pending `WM_PAINT` ahead of queued input once pointer moves have used their budget.
use super::super::pointer_paint::PaintTurn;
use bevy::{
    ecs::system::NonSendMarker,
    prelude::{App, Local, PreUpdate, Query, With, warn},
    window::{PrimaryWindow, RawHandleWrapper},
};
use raw_window_handle::RawWindowHandle;
use std::{cell::Cell, mem::zeroed, time::Instant};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, WM_MOUSEMOVE, WM_NCDESTROY,
            WM_NCMOUSEMOVE, WM_PAINT,
        },
    },
};

thread_local! {
    /// Lives outside the window because a frame run from the callback can destroy that window.
    static PAINT_TURN: Cell<PaintTurn> = const { Cell::new(PaintTurn::new()) };
}

/// Registers the callback on the event-loop thread, which owns the window.
pub(in crate::camera::focus) fn install(app: &mut App) {
    app.add_systems(PreUpdate, bind);
}

/// Subclasses the primary window once. Without the callback frames still run, as they do whenever
/// the message queue empties.
fn bind(
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    mut bound: Local<Option<isize>>,
    _event_loop_thread: NonSendMarker,
) {
    let Ok(raw) = windows.single() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = raw.get_window_handle() else {
        return;
    };
    if *bound == Some(handle.hwnd.get()) {
        return;
    }
    *bound = Some(handle.hwnd.get());
    let window = handle.hwnd.get() as HWND;
    let id = pointer_paint_proc as *const () as usize;
    if unsafe { SetWindowSubclass(window, Some(pointer_paint_proc), id, 0) } == 0 {
        warn!(
            "could not install the pointer paint handler: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Handles each pointer move first, so a frame it starts draws the newest cursor position.
unsafe extern "system" fn pointer_paint_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(window, Some(pointer_paint_proc), id);
        }
    }
    let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
    match message {
        WM_PAINT => with_turn(|turn| turn.frame_finished(Instant::now())),
        WM_MOUSEMOVE | WM_NCMOUSEMOVE
            if with_turn(|turn| turn.frame_due_after_move(Instant::now())) =>
        unsafe {
            dispatch_pending_paint(window);
        },
        _ => {}
    }
    result
}

/// Applies one change to this thread's paint turn.
fn with_turn<R>(change: impl FnOnce(&mut PaintTurn) -> R) -> R {
    PAINT_TURN.with(|cell| {
        let mut turn = cell.get();
        let result = change(&mut turn);
        cell.set(turn);
        result
    })
}

/// Dispatches this window's pending paint, if it has one, leaving queued input in order behind it.
///
/// The event loop ends its drain after a redraw and returns to its wait, so the frame also gets the
/// loop's normal end-of-batch work.
///
/// # Safety
/// Call on the thread that owns `window`.
unsafe fn dispatch_pending_paint(window: HWND) {
    let mut paint: MSG = unsafe { zeroed() };
    if unsafe { PeekMessageW(&mut paint, window, WM_PAINT, WM_PAINT, PM_REMOVE) } != 0 {
        unsafe {
            DispatchMessageW(&paint);
        }
    }
}
