//! Repeats a dressing of the player model whose inputs did not change.
//!
//! Dressing resolves the worn and held stacks and selects each one's equipment binding. It reads
//! the player and UI runtime, whose change ticks the caller vouches for, the world's local actor
//! and item registry, and the presentation's equipment catalogs and textures. While none changed
//! and no selection evaluates an owner condition, which may read per-frame owner state, a new
//! dressing would select the same gear, so the last one stands. The trim compositions it marked
//! are marked again before the frame's sweep, and held equipment's animation input follows the
//! frame.

use super::super::{UiPresentationRuntime, gui_models::trim_icons::TrimMarks};
use inventory::inventory_ledger::InventoryTarget;
use std::sync::Arc;

/// What the caller vouches did not change since the last dressing.
#[derive(Clone, Copy, Debug, Default)]
pub struct DressUnchanged {
    /// Neither the player nor the UI runtime changed.
    pub runtimes: bool,
    /// Nothing else changed the presentation.
    pub presentation: bool,
}

/// The last dressing's inputs besides the runtimes, and the trim compositions it marked.
pub(in crate::ui_runtime::presentation) struct DressRecord {
    key: Key,
    /// Whether a selection evaluated an owner condition: such a dressing is never repeated.
    conditional: bool,
    trims: TrimMarks,
}

/// What a dressing reads besides the player and UI runtime. Allocations are held, so a replaced
/// one never reuses an address.
struct Key {
    session: u64,
    actor: Option<client_world::ActorPresentationRevision>,
    registry: Option<Arc<()>>,
    equipment: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    pack: Option<Arc<assets::RuntimeEquipmentCatalog>>,
    selection: [Option<Arc<assets::RuntimeEntityAssets>>; 2],
    textures: Arc<render_model::UiRenderTextureArray>,
    show_custom_item_models: bool,
    models_enabled: bool,
}

/// Compares two retained allocations by identity, including absence.
fn same<T>(left: Option<&Arc<T>>, right: Option<&Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

impl Key {
    fn of(
        presentation: &UiPresentationRuntime,
        runtime: &crate::ui_runtime::UiRuntime,
        world: Option<client_world::ui_read::InventoryUi<'_>>,
    ) -> Self {
        Self {
            session: runtime.session_id(),
            actor: world.and_then(|world| world.local_player().presentation_revision()),
            registry: world.map(|world| Arc::clone(world.item_presentation_generation())),
            equipment: presentation.equipment_catalog.clone(),
            pack: presentation.gui_models.pack_equipment.source.clone(),
            selection: [false, true]
                .map(|pack| presentation.preview_equipment_selection.entities(pack)),
            textures: Arc::clone(&presentation.textures),
            show_custom_item_models: presentation.player_preview_gear.show_custom_item_models,
            models_enabled: presentation.gui_models.enabled,
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.session == other.session
            && self.actor == other.actor
            && same(self.registry.as_ref(), other.registry.as_ref())
            && same(self.equipment.as_ref(), other.equipment.as_ref())
            && same(self.pack.as_ref(), other.pack.as_ref())
            && same(self.selection[0].as_ref(), other.selection[0].as_ref())
            && same(self.selection[1].as_ref(), other.selection[1].as_ref())
            && Arc::ptr_eq(&self.textures, &other.textures)
            && self.show_custom_item_models == other.show_custom_item_models
            && self.models_enabled == other.models_enabled
    }
}

impl UiPresentationRuntime {
    /// [`Self::dress_player_preview`], repeating the last dressing when the caller vouches the
    /// runtimes are `unchanged` since the last call and nothing else the dressing reads changed
    /// either. With the presentation unchanged too, a
    /// repeat with the inventory closed and no shield or trim art waiting also skips the
    /// per-frame shield and trim observation: it would request the art already composed. It
    /// then also skips that frame's sweep of trim compositions nothing requested, which only
    /// frees them later.
    pub fn dress_player_preview_if_changed(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
        world: Option<client_world::ui_read::InventoryUi<'_>>,
        input: actor_animation::AttachableAnimationInput<'_>,
        resolve: impl Fn(&protocol::NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
        unchanged: DressUnchanged,
    ) -> bool {
        let key = Key::of(self, runtime, world);
        let record = self.gui_models.dress_record.take();
        let repeats = unchanged.runtimes
            && record
                .as_ref()
                .is_some_and(|record| !record.conditional && record.key.matches(&key));
        if let Some(record) = record.filter(|_| repeats) {
            if unchanged.presentation
                && !runtime.inventory_open()
                && !self.gui_models.shield_icons.dirty()
                && !self.gui_models.trim_icons.dirty()
            {
                self.follow_frame_input(input);
                #[cfg(debug_assertions)]
                self.check_quiet_dressing(player_runtime, runtime, world, input, &resolve, &record);
                self.gui_models.dress_record = Some(record);
                return false;
            }
            // What the dressing does every frame besides selecting gear.
            let mut changed = super::super::dynamic_textures::observe_session(self, key.session);
            self.gui_models
                .trim_icons
                .observe_session_items(runtime.session_items());
            changed |= super::super::publish::prepare_shield_art(
                player_runtime,
                runtime,
                self,
                world,
                &resolve,
            );
            if self.gui_models.trim_icons.mark_again(&record.trims) {
                self.follow_frame_input(input);
                #[cfg(debug_assertions)]
                self.check_repeated_dressing(player_runtime, runtime, world, input, &resolve);
                self.finish_trim_art();
                self.gui_models.dress_record = Some(record);
                return changed || self.gui_models.shield_icons.dirty();
            }
            // A composition it marked is gone: dressing again composes it.
            self.gui_models.dress_record = Some(record);
        }
        let marks = self
            .gui_models
            .dress_record
            .take()
            .map(|record| record.trims)
            .unwrap_or_default();
        self.gui_models.trim_icons.record_marks(marks);
        let changed = self.dress_player_preview(player_runtime, runtime, world, input, &resolve);
        #[cfg(test)]
        {
            self.gui_models.dressings += 1;
        }
        let trims = self.gui_models.trim_icons.take_marks();
        let conditional = self.dressing_is_conditional(player_runtime, runtime, &resolve);
        self.gui_models.dress_record = Some(DressRecord {
            key: Key::of(self, runtime, world),
            conditional,
            trims,
        });
        changed
    }

    /// Whether selecting any worn or held stack's binding evaluates an owner condition.
    fn dressing_is_conditional(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
        resolve: &impl Fn(&protocol::NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
    ) -> bool {
        let ledger = runtime.inventory_ledger(player_runtime);
        let base = self.equipment_catalog.as_deref();
        let pack = self.gui_models.pack_equipment.source.as_deref();
        (0..4)
            .map(|slot| ledger.target_stack(InventoryTarget::Armor(slot)))
            .chain([
                player_runtime
                    .selected_hotbar_slot()
                    .and_then(|slot| ledger.displayed_stack(slot)),
                ledger.target_stack(InventoryTarget::Offhand),
            ])
            .flatten()
            .filter_map(|stack| resolve(stack)?.identifier)
            .any(|identifier| {
                self.preview_equipment_selection
                    .conditional(base, pack, &identifier)
            })
    }

    /// Updates held equipment's animation input with this frame's values, as dressing sets it:
    /// the owner's own fields only change with the actor and its stacks.
    fn follow_frame_input(&mut self, input: actor_animation::AttachableAnimationInput<'_>) {
        if let Some(stored) = self.gui_models.live_equipment.input.as_mut() {
            stored.off_hand = input.off_hand;
            stored.held_item_scale = input.held_item_scale;
            stored.worn = input.worn;
            stored.worn_slot = input.worn_slot;
            stored.is_paperdoll = input.is_paperdoll;
            stored.is_in_ui = input.is_in_ui;
            stored.frame_alpha = input.frame_alpha;
            stored.delta_seconds = input.delta_seconds;
            stored.hand_charged = input.hand_charged;
        }
    }

    /// Runs the per-frame shield and trim observation a quiet repeat skipped and checks it
    /// changes nothing, then dresses again as [`Self::check_repeated_dressing`] does.
    #[cfg(debug_assertions)]
    fn check_quiet_dressing(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
        world: Option<client_world::ui_read::InventoryUi<'_>>,
        input: actor_animation::AttachableAnimationInput<'_>,
        resolve: &impl Fn(&protocol::NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
        record: &DressRecord,
    ) {
        let mut changed =
            super::super::dynamic_textures::observe_session(self, runtime.session_id());
        self.gui_models
            .trim_icons
            .observe_session_items(runtime.session_items());
        changed |= super::super::publish::prepare_shield_art(
            player_runtime,
            runtime,
            self,
            world,
            resolve,
        );
        assert!(
            self.gui_models.trim_icons.mark_again(&record.trims),
            "a quiet dressing's trim compositions are gone"
        );
        self.check_repeated_dressing(player_runtime, runtime, world, input, resolve);
        self.finish_trim_art();
        assert!(
            !changed && !self.gui_models.shield_icons.dirty(),
            "a quiet dressing's shield and trim observation changed what a frame draws"
        );
    }

    /// Dresses again and checks the repeated gear and held input are what a dressing produces.
    #[cfg(debug_assertions)]
    fn check_repeated_dressing(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &crate::ui_runtime::UiRuntime,
        world: Option<client_world::ui_read::InventoryUi<'_>>,
        input: actor_animation::AttachableAnimationInput<'_>,
        resolve: &impl Fn(&protocol::NetworkItemStack) -> Option<client_world::CanonicalItemStack>,
    ) {
        let digest = |presentation: &Self| {
            super::super::gui_models::debug_digest(&(
                &presentation.player_preview_gear,
                presentation.gui_models.wearing_elytra,
                presentation.gui_models.live_equipment.input,
            ))
        };
        let repeated = digest(self);
        self.dress_player_preview(player_runtime, runtime, world, input, resolve);
        assert_eq!(
            repeated,
            digest(self),
            "a repeated dressing differs from a fresh one"
        );
    }
}
