//! Per-mod limits of the `presentation` interface; the client enforces the same values.

/// Most annotated actors one mod retains through `presentation.set-annotations`.
pub const MAX_ANNOTATIONS: usize = 128;
/// Most text spans on one annotation surface.
pub const MAX_ANNOTATION_SPANS: usize = 4;
/// Most UTF-8 bytes in one annotation span.
pub const MAX_ANNOTATION_SPAN_BYTES: usize = 32;
/// Most tinted actors one mod retains through `presentation.set-actor-tints`.
pub const MAX_ACTOR_TINTS: usize = 64;
/// Most tooltip lines one mod retains, across every item.
pub const MAX_TOOLTIP_LINES: usize = 256;
/// Most mod lines one item's tooltip shows, across every mod.
pub const MAX_ITEM_TOOLTIP_LINES: usize = 8;
/// Most UTF-8 bytes in one tooltip line.
pub const MAX_TOOLTIP_LINE_BYTES: usize = 96;
/// Most clickable chat spans one mod retains.
pub const MAX_CHAT_ACTIONS: usize = 32;
/// Most UTF-8 bytes of chat text one action matches.
pub const MAX_CHAT_ACTION_TEXT_BYTES: usize = 96;
/// Most UTF-8 bytes in one chat action's reported id.
pub const MAX_CHAT_ACTION_ID_BYTES: usize = 48;
