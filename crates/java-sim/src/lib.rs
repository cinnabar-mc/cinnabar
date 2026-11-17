//! Java Edition 1.7.10 local-player physics.
//!
//! The crate reproduces the 1.7.10 client's player tick: input handling, sprinting, flying,
//! liquid, ladder and web movement, block collision with stepping and sneak edges, and the
//! position packets the client reports. It is independent of the Bedrock simulator in `sim`:
//! callers supply a [`World`] of 1.7.10 block ids and receive positions and packets in the
//! game's own `f64` arithmetic.

pub mod block;
pub mod math;
pub mod player;
mod world;

pub use block::Block;
pub use math::Aabb;
pub use player::{Abilities, Effects, Input, Packet, Player, Status};
pub use world::{WORLD_HEIGHT, World};
