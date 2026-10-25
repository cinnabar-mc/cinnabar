//! Synchronous UI observations borrowed from the admitted world, without terrain scheduling.

use std::sync::Arc;

use crate::{ActorSnapshot, CanonicalItemStack, EffectiveItemUse, PlayerProfile, WorldAuthority};
use actor_animation::{ActorRigSnapshot, BoneTransform};
use world::{BlockEntityKey, BlockPos, NbtCompound};

/// The local actor and evaluated appearance at one caller-selected observation point.
/// References retain the owner's profile, skin and pack allocations without copying world state.
#[derive(Clone, Copy)]
pub struct LocalPlayerUi<'a> {
    authority: &'a WorldAuthority,
}

/// Container, mount and item facts needed while capturing an inventory/HUD frame.
#[derive(Clone, Copy)]
pub struct InventoryUi<'a> {
    authority: &'a WorldAuthority,
}

/// Actor labels and ride relationships needed for nametag and scoreboard projection.
#[derive(Clone, Copy)]
pub struct NametagUi<'a> {
    authority: &'a WorldAuthority,
}

impl WorldAuthority {
    /// Borrows the local profile and completed full-body evaluation without advancing animation.
    pub fn local_player_ui(&self) -> LocalPlayerUi<'_> {
        LocalPlayerUi { authority: self }
    }

    /// Borrows only the world queries used by inventory and station presentation.
    pub fn inventory_ui(&self) -> InventoryUi<'_> {
        InventoryUi { authority: self }
    }

    /// Borrows only label and passenger observations, without streaming or mutation access.
    pub fn nametag_ui(&self) -> NametagUi<'_> {
        NametagUi { authority: self }
    }
}

impl<'a> LocalPlayerUi<'a> {
    /// Identifies the local actor even while its appearance is not ready.
    pub fn runtime_id(self) -> u64 {
        self.authority.local_player_runtime_id()
    }

    /// Identifies the owning actor session for fire, emitter and skin cache fences.
    pub fn session_id(self) -> u64 {
        self.authority.actor_session_id()
    }

    /// Borrows the admitted local actor without creating a presentation copy.
    pub fn actor(self) -> Option<&'a ActorSnapshot> {
        self.authority.actor(self.runtime_id())
    }

    /// Borrows the admitted profile and its shared skin allocation.
    pub fn profile(self) -> Option<&'a PlayerProfile> {
        self.authority.actor_player_profile(self.runtime_id())
    }

    /// Borrows the completed local rig while preserving its pack and skin identity.
    pub fn rig(self) -> Option<ActorRigSnapshot<'a>> {
        self.authority.actor_rig(self.runtime_id())
    }

    /// Borrows the completed full-body pose, independently of first-person hand evaluation.
    pub fn pose(self) -> Option<&'a [BoneTransform]> {
        self.authority.actor_ui_pose(self.runtime_id())
    }
}

impl<'a> InventoryUi<'a> {
    /// Captures the local actor's current admitted profile and evaluated appearance.
    pub fn local_player(self) -> LocalPlayerUi<'a> {
        self.authority.local_player_ui()
    }

    /// Reads a global block position in the current dimension, preserving lazy NBT sharing.
    pub fn block_entity_compound(self, position: BlockPos) -> Option<Arc<NbtCompound>> {
        let key = BlockEntityKey::new(
            self.authority.current_dimension(),
            position.x,
            position.y,
            position.z,
        );
        self.authority.terrain().block_entity(key)?.parsed()
    }

    /// Resolves a nonempty custom title from the same admitted block entity as its screen.
    pub fn block_entity_custom_name(self, position: BlockPos) -> Option<String> {
        self.block_entity_compound(position)?
            .string("CustomName")
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }

    /// Borrows a window's actor identity; callers decide which station fields to display.
    pub fn container_actor(self, unique_id: i64) -> Option<&'a ActorSnapshot> {
        self.authority.actor_by_unique_id(unique_id)
    }

    /// Prefers the actor's filtered container title over its name tag.
    pub fn actor_container_name(self, unique_id: i64) -> Option<Arc<str>> {
        self.authority.actor_container_name(unique_id)
    }

    /// Reads admitted mount equipment capabilities, retaining unresolved values.
    pub fn actor_mount_equipment(self, unique_id: i64) -> assets::entity::mount::MountEquipment {
        self.authority.actor_mount_equipment(unique_id)
    }

    /// Reads well-formed mount health for the current HUD observation.
    pub fn actor_health_by_unique(self, unique_id: i64) -> Option<(f32, f32)> {
        self.authority.actor_health_by_unique(unique_id)
    }

    /// Reports whether the mount exposes a jump bar, without changing jump input state.
    pub fn mount_has_jump(self, unique_id: i64) -> bool {
        self.authority.actor_has_attribute_by_unique(
            unique_id,
            protocol_ids::Attribute::HorseJumpStrength.wire_name(),
        )
    }

    /// Resolves an inventory stack against the admitted item registry.
    pub fn canonical_item_stack(
        self,
        stack: &protocol::NetworkItemStack,
    ) -> Option<CanonicalItemStack> {
        self.authority.canonical_item_stack(stack)
    }

    /// Retains unknown armor protection rather than inventing a display value.
    pub fn item_armour(self, network_id: i32) -> assets::item::armour::ItemArmour {
        self.authority.item_armour(network_id)
    }

    /// Reads registry durability for an inventory stack's display correction.
    pub fn item_durability(self, network_id: i32) -> assets::item::durability::ItemDurability {
        self.authority.item_durability(network_id)
    }

    /// Reads use timing for the local player's evaluated equipment preview.
    pub fn item_use_profile(self, network_id: i32) -> EffectiveItemUse {
        self.authority.item_use_profile(network_id)
    }

    /// Borrows the registry identity token so retained projections keep its allocation alive.
    pub fn item_presentation_generation(self) -> &'a Arc<()> {
        self.authority.item_presentation_generation()
    }
}

impl<'a> NametagUi<'a> {
    /// Iterates admitted remote actors without allocating a second actor collection.
    pub fn remote_actors(self) -> impl Iterator<Item = &'a ActorSnapshot> {
        self.authority.remote_actors()
    }

    /// Borrows a selected candidate after distance ordering and admission.
    pub fn actor(self, runtime_id: u64) -> Option<&'a ActorSnapshot> {
        self.authority.actor(runtime_id)
    }

    /// Identifies the local rider when suppressing a mount's appended score.
    pub fn local_player_unique_id(self) -> i64 {
        self.authority.local_player_unique_id()
    }

    /// Reads the actor's current mount relationship.
    pub fn ridden_unique_id(self, unique_id: i64) -> Option<i64> {
        self.authority.ridden_unique_id(unique_id)
    }

    /// Shares the synchronized tag text, preserving its allocation and formatting.
    pub fn actor_name_tag(self, unique_id: i64) -> Option<Arc<str>> {
        self.authority.actor_name_tag(unique_id)
    }

    /// Shares the retained scoreboard display name for one required owner.
    pub fn actor_display_name(self, unique_id: i64) -> Option<Arc<str>> {
        self.authority.actor_display_name(unique_id)
    }
}

#[cfg(test)]
mod tests;
