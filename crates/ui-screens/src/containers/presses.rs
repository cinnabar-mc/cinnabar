//! What a press on an engine-drawn container does with the held stack or the slot under it.

use json_ui::{HitKind, hit_test};
use ui_engine::frame::EngineFrame;

/// The press targets that drop the held stack, as vanilla's `drop_item_panel`
/// routes them.
const CURSOR_DROPS: [&str; 2] = ["button.cursor_drop_all", "button.cursor_drop_one"];

/// The press target vanilla's item scroll panels map a primary press on their background to.
const DESTROY_SELECTION: &str = "button.destroy_selection";

/// What a primary press that reaches no slot does with the held stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldStackPress {
    /// The stack is dropped into the world.
    Drop,
    /// The stack is destroyed where the screen allows it, as on the creative item panel.
    Destroy,
}

/// What a press at `gui` on the engine-drawn container does with the held stack.
/// The press drops when it reaches no control, since the screen itself maps it
/// to a cursor drop, or reaches a drop panel; it destroys on a panel mapping it to
/// `button.destroy_selection`, such as the creative and recipe book item grid. Every
/// other `common.root_panel` section, such as the inventory or creative hotbar,
/// claims presses anywhere on its background, as do its buttons. A disabled control
/// claims the press and does nothing with it.
pub fn engine_held_stack_press(frame: &EngineFrame, gui: [f32; 2]) -> Option<HeldStackPress> {
    let point = [f64::from(gui[0]), f64::from(gui[1])];
    // A scroll view without its own press mapping takes the wheel, not the press: the press
    // reaches the input panel around it, as the item grid's background does.
    let region = frame.hits.iter().rev().find(|region| {
        region.contains(point)
            && match region.kind {
                HitKind::ScrollView | HitKind::Panel => region.pressed.is_some(),
                _ => true,
            }
    });
    let Some(region) = region else {
        return Some(HeldStackPress::Drop);
    };
    if !region.enabled {
        return None;
    }
    match region.pressed.as_deref() {
        Some(target) if CURSOR_DROPS.contains(&target) => Some(HeldStackPress::Drop),
        Some(DESTROY_SELECTION) => Some(HeldStackPress::Destroy),
        _ => None,
    }
}

/// The input a Shift-click raises on a container slot.
const MENU_AUTO_PLACE: &str = "button.menu_auto_place";

/// Where vanilla's creative-layout hotbar slots route [`MENU_AUTO_PLACE`].
const AUTO_DESTROY: &str = "button.container_auto_destroy";

/// Whether a Shift-click at `gui` on the engine-drawn container deletes the slot's
/// stack rather than moving it: the control under the press maps
/// `button.menu_auto_place` to `button.container_auto_destroy`, as the creative
/// layout's hotbar slots do. Other slots, including the survival layout's
/// hotbar, keep moving the stack.
pub fn engine_auto_place_destroys(frame: &EngineFrame, gui: [f32; 2]) -> bool {
    hit_test(&frame.hits, [f64::from(gui[0]), f64::from(gui[1])]).is_some_and(|region| {
        region.enabled && region.input.pressed_target(MENU_AUTO_PLACE) == Some(AUTO_DESTROY)
    })
}
