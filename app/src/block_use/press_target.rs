//! Lets presses resolved before block use see whether the aimed block takes them.
use super::*;
use bevy::prelude::Res;
use gameplay::block_use::PickResult;

/// The retained frame pick and the world it was taken in, for input read before block use.
#[derive(SystemParam)]
pub(crate) struct UsePressTarget<'w> {
    runtime: Option<Res<'w, BlockUseRuntime>>,
    client_world: Option<Res<'w, ClientWorld>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
}

impl UsePressTarget<'_> {
    /// Whether a use press now goes to the aimed block's own use instead of the held item's,
    /// so block use sends it and the item's use, such as opening a book, never starts.
    /// Missing picks, misses, actors and unloaded cells leave the press to the item.
    pub(crate) fn block_takes_press(&self, movement: &MovementTicker) -> bool {
        let (Some(runtime), Some(client_world), Some(collisions)) = (
            self.runtime.as_deref(),
            self.client_world.as_deref(),
            self.collisions.as_deref(),
        ) else {
            return false;
        };
        let (Some(stream), Some(state)) =
            (client_world.stream.as_ref(), movement.build_action_state())
        else {
            return false;
        };
        let Some(PickResult::Block(block)) =
            runtime.pick(movement).and_then(|pick| pick.hits.ordinary)
        else {
            return false;
        };
        if block.indirect {
            return false;
        }
        let mode = stream.network_id_mode();
        let world = PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(mode),
            stream.current_dimension(),
            Default::default(),
        );
        world
            .primary_runtime_id(block.position)
            .is_ok_and(|runtime_id| {
                gameplay::block_use::block_takes_use_press(
                    collisions.block_facts(mode, runtime_id),
                    state.sneaking,
                )
            })
    }
}
