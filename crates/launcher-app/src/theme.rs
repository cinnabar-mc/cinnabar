//! The launcher's brand palette: warm near-black surfaces under the logo's cinnabar red.
//!
//! The OreUI theme supplies the type scale and spacing; these colours give the launcher its own
//! identity, taken from `assets/branding/title.png`.

use ui::oreui_theme::Rgba;

/// The logo's red, used for the Play button, the active page and focus accents.
pub const ACCENT: Rgba = [0xe2, 0x45, 0x2f, 0xff];
pub const ACCENT_HOVER: Rgba = [0xf0, 0x5a, 0x40, 0xff];
pub const ACCENT_PRESSED: Rgba = [0xb8, 0x33, 0x1f, 0xff];
/// The raised accent button's lower bevel.
pub const ACCENT_SHADOW: Rgba = [0x6e, 0x1a, 0x10, 0xff];
/// The raised accent button's upper highlight.
pub const ACCENT_SPECULAR: Rgba = [0xff, 0xff, 0xff, 0x40];

pub const SIDEBAR: Rgba = [0x15, 0x10, 0x11, 0xff];
pub const BAR: Rgba = [0x1b, 0x14, 0x15, 0xf2];
/// Cards and panels over the panorama.
pub const SURFACE: Rgba = [0x23, 0x1a, 0x1b, 0xe6];
pub const SURFACE_HOVER: Rgba = [0x30, 0x24, 0x25, 0xf0];
pub const SURFACE_RAISED: Rgba = [0x3a, 0x2c, 0x2d, 0xff];
pub const BORDER: Rgba = [0x3a, 0x2b, 0x2c, 0xff];
pub const DIVIDER: Rgba = [0x2c, 0x21, 0x22, 0xff];

pub const TEXT: Rgba = [0xf5, 0xef, 0xee, 0xff];
pub const TEXT_DIM: Rgba = [0xbc, 0xaf, 0xad, 0xff];
pub const TEXT_FAINT: Rgba = [0x85, 0x79, 0x7a, 0xff];

/// Status colours for prepared and pending resources.
pub const READY: Rgba = [0x8f, 0xd6, 0x94, 0xff];
pub const PENDING: Rgba = [0xf2, 0xb8, 0x4b, 0xff];

/// The keyboard and controller focus outline.
pub const FOCUS: Rgba = [0xff, 0xff, 0xff, 0xff];
/// The scrim behind the failure modal.
pub const SCRIM: Rgba = [0x08, 0x05, 0x06, 0xb4];

/// `color` with its alpha replaced by `alpha`.
pub const fn with_alpha(color: Rgba, alpha: u8) -> Rgba {
    [color[0], color[1], color[2], alpha]
}
