//! Built-in typography, colors and handle geometry for touch editor chrome.

/// Largest label scale that fits the twelve-GUI-pixel editor caption band.
const MAX_TEXT_SCALE: f32 = 0.8;

/// Keeps the shared editor chrome at its built-in appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EditorStyle {
    /// Draws one measured name-and-scale caption for the selected widget.
    pub selection_caption: bool,
    /// Text on the selected-color caption background.
    pub caption_text: [f32; 4],
    /// Host-owned face used by editor labels and distance annotations.
    pub font: ui::mod_hud::CardFont,
    /// Label scale, independent of each widget's gameplay scale.
    pub text_scale: f32,
    /// Top toolbar band, in GUI pixels, reserved from selection and distance captions.
    pub top_clearance: f32,
    /// Outline of a hovered card that is not selected.
    pub outline: [f32; 4],
    /// Outline of the selected card.
    pub selected: [f32; 4],
    /// Resize handle at rest.
    pub handle: [f32; 4],
    /// Resize handle under the pointer.
    pub handle_hover: [f32; 4],
    /// Resize handle while it is dragged.
    pub handle_pressed: [f32; 4],
    /// Corner radius of the five-pixel resize handle mark.
    pub handle_radius: f32,
    /// Temporary alignment guide shown while dragging.
    pub guide: [f32; 4],
    /// Built-in snapping grid lines, drawn only without an extension surface.
    pub grid: [f32; 4],
    /// Fill of the distance measurement pills around the selected card.
    pub measurement: [f32; 4],
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            selection_caption: false,
            caption_text: [0.04, 0.04, 0.04, 1.],
            font: ui::mod_hud::CardFont::Native,
            text_scale: MAX_TEXT_SCALE,
            top_clearance: 0.,
            outline: [1., 1., 1., 0.55],
            selected: [0.3, 0.85, 1., 1.],
            handle: [0.3, 0.85, 1., 1.],
            handle_hover: [1.; 4],
            handle_pressed: [0.2, 0.65, 1., 1.],
            handle_radius: 0.,
            guide: [0.2, 0.85, 1., 0.8],
            grid: [1., 1., 1., 0.09],
            measurement: [0.055, 0.065, 0.08, 0.97],
        }
    }
}
