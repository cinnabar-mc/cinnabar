//! The server half: a guest of the `server` world, which the Experience runtime runs as
//! `server.wasm`, generated from `wit/server/server.wit`.
//!
//! Implement [`Experience`] on a type and export it with
//! [`export_experience!`](crate::export_experience). Paired projects use `#[entry::server]`
//! and one `#[on_message]` method per request. [`Callback::send`] checks the reply's
//! direction and fields and uses its declared delivery mode. Message and epoch callbacks
//! may read and write the block of the player's focus, [`Callback::focus`].
//!
//! Server WIT 0.5 declares blocks with states, placement traits, visuals and network membership,
//! and items, through [`Experience::registration`]; a guest that only has plain cube blocks
//! implements [`Experience::register`] instead. Its callbacks add [`Callback::block_states`],
//! [`Callback::set_block_state`], [`Callback::network`], [`Callback::inventory`],
//! [`Callback::set_slot`] and [`Callback::drop_item`].
//!
//! Server WIT 0.6 adds the server's crafting recipes, [`Callback::crafting_result`], and
//! scheduled updates: [`Callback::schedule_update`] stages one, and
//! [`Experience::on_scheduled_update`] runs when it is due.
//!
//! Server WIT 0.11 adds registered entities and ordered entity callbacks through
//! [`Experience::entity_definitions`] and [`Experience::on_entity`].
//!
//! Server WIT 0.10 adds cell overlays: [`Experience::overlays`] declares the blocks that carry
//! them and the overlay types, [`Callback::overlays`] and [`Callback::set_overlays`] read and
//! stage a cell's, and [`Experience::on_interact`] gets the click position and the overlay it
//! picks.

/// Bindings generated from `wit/server/server.wit`.
// The canonical-ABI shims for `on-place` and `on-break` take the flattened
// `block-change` record, which exceeds Clippy's argument limit.
#[allow(clippy::too_many_arguments)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit/server",
        world: "server",
        pub_export_macro: true,
    });
}

pub use bindings::cinnabar::experience_server::{
    diagnostics::log,
    types::{
        BlockChange, BlockDef, BlockPos, BlockState, BlockType, BoneVisibility, CallbackInfo,
        ChangeCause, Condition, Crafted, DimensionType, EntityAppearance, EntityArt, EntityBox,
        EntityDefinition, EntityEvent, EntityHand, EntityHandle, EntityInput, EntityInteraction,
        EntityProperty, EntityPropertyDef, EntityPropertyId, EntityRider, EntityRotation,
        EntitySeat, EntityState, EntityTypeId, Face, Flipbook, Generator, GuestError, Inventory,
        ItemDef, ItemKey, ItemStack, LogLevel, Material, Mining, Network, NewStack, Overlay,
        OverlayItem, OverlayLook, OverlayRegistration, OverlayType, OverlayVisual, Permutation,
        Pixel, PixelBox, PlacementStates, PlayerEvent, PlayerId, PlayerLocation, PlayerTransition,
        Position, QuarterTurns, Registration, RenderMethod, Scalar, ScheduleHandle, SimulationStep,
        StateDef, StateTest, StateValue, StateValues, StoreKey, StoreScope, TextureBinding,
        ValueNode, Visual, WorldDefinition, WorldDefinitionId, WorldError, WorldHandle,
        WorldInstance, WorldInstanceId, WorldRegistration, WorldSettings,
    },
};
pub use nodes::{nodes, values};

mod callback;
pub use callback::Callback;

/// ABI adapters used by generated entry points and low-level protocol fixtures.
#[doc(hidden)]
pub mod __private;

mod nodes;

/// The block type of a plain block: a stateless cube with `def`'s textures, in no network.
pub fn cube(def: BlockDef) -> BlockType {
    BlockType {
        def,
        states: Vec::new(),
        placement: PlacementStates::empty(),
        visual: None,
        permutations: Vec::new(),
        network: false,
    }
}

/// One server Experience, mirroring the exports of the `server` world.
///
/// Implement [`Experience::registration`], or [`Experience::register`] for plain cube blocks
/// alone; the callbacks default to `Ok(())`, which accepts the event without staging anything.
pub trait Experience {
    /// Declares the entity types whose identities appear in the shared module.
    fn entity_definitions() -> Result<Vec<EntityDefinition>, GuestError> {
        Ok(Vec::new())
    }
    /// Receives interactions, attacks, seat changes, input and entity removal in queue order.
    fn on_entity(
        _ctx: &Callback,
        _handle: EntityHandle,
        _event: EntityEvent,
    ) -> Result<(), GuestError> {
        Ok(())
    }
    /// Receives one admitted fixed-step callback for each live world instance. A world queues no
    /// further tick while one is pending, so a busy Experience skips steps and gets the latest.
    fn on_tick(_ctx: &Callback, _step: SimulationStep) -> Result<(), GuestError> {
        Ok(())
    }
    /// Receives a scheduled callback; repeating handles remain cancellable.
    fn on_timer(
        _ctx: &Callback,
        _handle: ScheduleHandle,
        _step: SimulationStep,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Declares world definitions and instances to open before admitting players.
    fn world_definitions() -> Result<WorldRegistration, GuestError> {
        Ok(WorldRegistration {
            definitions: Vec::new(),
            startup: Vec::new(),
        })
    }
    /// Observes an admitted join, departure, transfer, or coalesced movement.
    fn on_player(
        _ctx: &Callback,
        _player: PlayerId,
        _event: PlayerEvent,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Returns the generated shared contract when this server belongs to a mod project.
    fn registry() -> Option<&'static crate::message::Registry> {
        None
    }

    /// Declares the plain cube blocks this Experience owns. It runs once, at startup, through
    /// the default [`Experience::registration`].
    fn register() -> Result<Vec<BlockDef>, GuestError> {
        Ok(Vec::new())
    }

    /// Declares this Experience's blocks and items. It runs once, at startup; by default it
    /// declares [`Experience::register`]'s blocks as [`cube`]s and no items.
    fn registration() -> Result<Registration, GuestError> {
        Ok(Registration {
            blocks: Self::register()?.into_iter().map(cube).collect(),
            items: Vec::new(),
        })
    }

    /// Declares the blocks that may carry overlays and this Experience's overlay types. It runs
    /// once, at startup, after [`Experience::registration`]; by default it declares none.
    fn overlays() -> Result<OverlayRegistration, GuestError> {
        Ok(OverlayRegistration {
            hosts: Vec::new(),
            types: Vec::new(),
        })
    }

    /// Handles `on-place` for one of this Experience's blocks.
    fn on_place(_ctx: &Callback, _change: BlockChange) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-break` for one of this Experience's blocks.
    fn on_break(_ctx: &Callback, _change: BlockChange) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-interact`: `player` used the block at `pos`, clicking `clicked_face`. `hit`
    /// is where the click met the block, in pixels, when the client sent one within the cell,
    /// and `target` the slot of the block's overlay that `hit` picks.
    fn on_interact(
        _ctx: &Callback,
        _player: PlayerId,
        _pos: BlockPos,
        _clicked_face: Face,
        _hit: Option<Pixel>,
        _target: Option<u8>,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-neighbor-changed`: the block at `neighbor`, next to `pos`, changed.
    fn on_neighbor_changed(
        _ctx: &Callback,
        _pos: BlockPos,
        _neighbor: BlockPos,
    ) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `epoch`: `player`'s client part moved to a new world epoch, such as another
    /// dimension, and kept running, so it may have missed what was sent before; resend its state.
    /// `ctx` has the snapshot of `player`'s focus as a message handler does.
    fn epoch(_ctx: &Callback, _player: PlayerId) -> Result<(), GuestError> {
        Ok(())
    }

    /// Handles `on-scheduled-update`: an update [`Callback::schedule_update`] staged for this
    /// Experience's block at `pos` is due. `ctx` has the snapshot an interaction with the block
    /// would have.
    fn on_scheduled_update(_ctx: &Callback, _pos: BlockPos) -> Result<(), GuestError> {
        Ok(())
    }
}

/// Implements the generated [`bindings::Guest`] for `$ty` by delegating to its
/// [`Experience`] impl, then exports `$ty` as the component's `server` world.
#[macro_export]
macro_rules! export_experience {
    ($ty:ident) => {
        impl $crate::server::bindings::Guest for $ty {
            fn entity_definitions() -> ::core::result::Result<::std::vec::Vec<$crate::server::EntityDefinition>, $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::entity_definitions()
            }
            fn on_entity(ctx: &$crate::server::__private::RawCallback, handle: $crate::server::EntityHandle, event: $crate::server::EntityEvent) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_entity(&$crate::server::Callback::from_raw(ctx), handle, event)
            }
            fn on_tick(ctx: &$crate::server::__private::RawCallback, step: $crate::server::SimulationStep) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_tick(&$crate::server::Callback::from_raw(ctx), step)
            }
            fn on_timer(ctx: &$crate::server::__private::RawCallback, handle: $crate::server::ScheduleHandle, step: $crate::server::SimulationStep) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_timer(&$crate::server::Callback::from_raw(ctx), handle, step)
            }

            fn world_definitions() -> ::core::result::Result<$crate::server::WorldRegistration, $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::world_definitions()
            }
            fn overlays() -> ::core::result::Result<$crate::server::OverlayRegistration, $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::overlays()
            }
            fn on_player(ctx: &$crate::server::__private::RawCallback, player: $crate::server::PlayerId, event: $crate::server::PlayerEvent) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_player(&$crate::server::Callback::from_raw(ctx), player, event)
            }

            fn register() -> ::core::result::Result<
                $crate::server::Registration,
                $crate::server::GuestError,
            > {
                <$ty as $crate::server::Experience>::registration()
            }

            fn on_place(
                ctx: &$crate::server::__private::RawCallback,
                change: $crate::server::BlockChange,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_place(&$crate::server::Callback::from_raw(ctx), change)
            }

            fn on_break(
                ctx: &$crate::server::__private::RawCallback,
                change: $crate::server::BlockChange,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_break(&$crate::server::Callback::from_raw(ctx), change)
            }

            fn on_interact(
                ctx: &$crate::server::__private::RawCallback,
                player: $crate::server::PlayerId,
                pos: $crate::server::BlockPos,
                clicked_face: $crate::server::Face,
                hit: ::core::option::Option<$crate::server::Pixel>,
                target: ::core::option::Option<u8>,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_interact(&$crate::server::Callback::from_raw(ctx), player, pos, clicked_face, hit, target)
            }

            fn on_neighbor_changed(
                ctx: &$crate::server::__private::RawCallback,
                pos: $crate::server::BlockPos,
                neighbor: $crate::server::BlockPos,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_neighbor_changed(&$crate::server::Callback::from_raw(ctx), pos, neighbor)
            }

            fn client_message(
                ctx: &$crate::server::__private::RawCallback,
                player: $crate::server::PlayerId,
                channel: ::std::string::String,
                schema: u16,
                payload: ::std::vec::Vec<$crate::server::ValueNode>,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                $crate::server::__private::receive::<$ty>(ctx, player, channel, schema, payload)
            }

            fn epoch(
                ctx: &$crate::server::__private::RawCallback,
                player: $crate::server::PlayerId,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::epoch(&$crate::server::Callback::from_raw(ctx), player)
            }

            fn on_scheduled_update(
                ctx: &$crate::server::__private::RawCallback,
                pos: $crate::server::BlockPos,
            ) -> ::core::result::Result<(), $crate::server::GuestError> {
                <$ty as $crate::server::Experience>::on_scheduled_update(&$crate::server::Callback::from_raw(ctx), pos)
            }
        }

        $crate::server::bindings::export!($ty with_types_in $crate::server::bindings);
    };
}

mod stores;
pub use stores::{Persistent, StateError};
