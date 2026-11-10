//! The client's graphics mode, which selects the entity material list server packs compile with:
//! Bedrock loads `materials/fancy.json` with Fancy Graphics and `materials/sad.json` without.

use std::sync::atomic::{AtomicBool, Ordering};

/// Fancy Graphics, on unless the client turned it off.
static FANCY_GRAPHICS: AtomicBool = AtomicBool::new(true);

/// Sets the graphics mode later pack compiles use.
pub fn set_fancy_graphics(fancy: bool) {
    FANCY_GRAPHICS.store(fancy, Ordering::Relaxed);
}

/// Whether pack compiles use Fancy Graphics' entity material list.
#[must_use]
pub fn fancy_graphics() -> bool {
    FANCY_GRAPHICS.load(Ordering::Relaxed)
}
