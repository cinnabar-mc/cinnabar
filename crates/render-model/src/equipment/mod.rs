//! Shared equipment geometry for world rendering and inventory previews.

mod attachable;
pub mod blocks;
mod display;
mod render_offsets;
pub mod shield;
#[cfg(test)]
mod tests;

pub use attachable::{BoneChannels, attach, authored_rotation};
pub use display::{
    ItemDisplay, attach_to_bone, held_block_display, held_block_display_for_hand,
    held_sprite_display, held_sprite_display_for_hand, sprite_item_transform,
};
pub use render_offsets::{
    native_sprite_from_held, offset_display, offset_first_person_lead, offset_follows_actor_scale,
    offset_held_display, offset_sprite_matrix,
};
