//! Semantic controller actions over container cells: what each logical action means
//! for a target, the hint it shows, and its dispatch onto the existing inventory paths.
//!
//! Availability here is *eligibility* for a prompt, derived from the retained ledger view.
//! It never promises execution: target identity settlement, packet capacity, stack limits and
//! slot restrictions are decided by the ledger when the action is dispatched, and a refusal
//! is reported as [`ControllerActionOutcome::Rejected`].
//!
//! Actions are logical IDs, not physical buttons. Which button triggers which action,
//! and any repeat or double-press timing, is decided by the caller from captured
//! Bedrock evidence; nothing here assumes a face-button layout.

use inventory::inventory_ledger::{DropSource, InventoryGestureError};
use protocol::{NetworkItemStack, WindowKind};
use ui_screens::container_actions::InventoryCellHit;
use ui_screens::container_kinds::{self, Cell};

use super::navigation::Selection;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::inventory_actions::gesture_target;
use crate::ui_runtime::inventory_drag::PointerAction;

/// Collection-name suffix the screen tables use for take-only output cells.
const OUTPUT_COLLECTION_SUFFIX: &str = "output_items";

/// Whether the open window's screen table lists `slot` in a collection chosen by `pick`.
fn screen_lists(kind: Option<WindowKind>, slot: u8, pick: impl Fn(&str) -> bool) -> bool {
    kind.and_then(container_kinds::window_kind)
        .is_some_and(|screen| {
            screen
                .collections
                .iter()
                .any(|(name, cells)| pick(name) && cells.contains(&Cell::Storage(slot)))
        })
}

/// One logical container action, independent of the physical control that raised it.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ContainerControllerAction {
    /// Take, place, merge or swap the whole stack; activate a control; craft once.
    Primary,
    /// Take the larger half, or place one item from the carried stack.
    HalfOrPlaceOne,
    /// Move the target through the existing quick-transfer routing; craft all on a result.
    QuickTransfer,
    /// Pull compatible stacks into the carried stack.
    Gather,
    /// Drop one item from the carried stack, or from an occupied cell when nothing is carried.
    DropOne,
    /// Drop the whole carried stack, or the whole occupied cell when nothing is carried.
    DropAll,
}

impl ContainerControllerAction {
    /// Every action, in prompt order.
    pub const ALL: [Self; 6] = [
        Self::Primary,
        Self::HalfOrPlaceOne,
        Self::QuickTransfer,
        Self::Gather,
        Self::DropOne,
        Self::DropAll,
    ];
}

/// What kind of cell a hit addresses, which decides the roles actions may take.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TargetRole {
    /// An ordinary player, storage, armor, offhand or crafting-input cell.
    Item,
    /// The created-output cell of a crafting-style screen.
    CraftOutput,
    /// A take-only output cell that the open screen's table names as an output.
    StationOutput,
    /// A creative catalog cell.
    CreativeCatalog,
    /// A recipe-book or creative-group entry.
    RecipeBook,
    /// A button, tab, search field or other non-item control.
    Control,
}

/// The wording a prompt shows for an available action.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionIntent {
    Take,
    TakeHalf,
    Place,
    PlaceOne,
    /// A whole-stack move onto an occupied cell; whether it merges or swaps is the
    /// ledger's decision, so no finer wording is claimed.
    Move,
    TakeOutput,
    CraftOnce,
    CraftAll,
    QuickTransfer,
    Gather,
    DropOne,
    DropAll,
    ToggleCrafterSlot,
    Activate,
}

/// Why an action is not offered or not dispatched. None of these touches the ledger.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionUnavailable {
    /// The action does not exist for this kind of target.
    NotApplicable,
    /// The target and the carried stack are both empty.
    NothingToAct,
    /// Gather needs a carried stack.
    CursorEmpty,
    /// The cell only gives items out; nothing can be placed in it.
    OutputOnly,
    /// The Bedrock mapping for this target has not been witnessed, so none is claimed.
    UnresolvedMapping,
    /// A text field owns the input.
    TextEntry,
    /// No container screen is open.
    ScreenClosed,
    /// The open window is not the one the target was resolved against.
    StaleWindow,
    /// The focused semantic target or its layout changed since the input was resolved.
    StaleTarget,
}

/// When an action's real admission is decided.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Admission {
    /// Only at dispatch: the ledger re-reads the cell, settles stack identities and checks
    /// queue capacity, stack limits and slot restrictions. A hint does not guarantee success.
    AtDispatch,
}

/// How an action is triggered. Held or repeated input is never implied.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Trigger {
    /// One admitted operation per press edge.
    Press,
}

/// Whether one action is offered for one target, and how it behaves.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ActionCapability {
    pub action: ContainerControllerAction,
    pub availability: Result<ActionIntent, ActionUnavailable>,
    pub trigger: Trigger,
    pub admission: Admission,
}

/// An available action and the wording to show for it.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ActionHint {
    pub action: ContainerControllerAction,
    pub intent: ActionIntent,
}

/// What the ledger says is open. A personal inventory has no window facts, so a stamp
/// cannot tell one opening of it from the next; [`ScreenToken`] adds the caller's generation.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct WindowStamp {
    kind: Option<WindowKind>,
    position: Option<[i32; 3]>,
    generation: Option<u64>,
    actor: Option<i64>,
}

/// Identity of one opening of a container screen, captured when a target is resolved.
///
/// The coordinator owns `screen_generation` and must change it on every open and close of
/// the container UI, including personal-inventory reopen. The window facts only add
/// protection against a replaced server window.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ScreenToken {
    screen_generation: u64,
    window: WindowStamp,
}

/// What dispatching an action did.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ControllerActionOutcome {
    /// The operation was admitted. `request_id` is the ledger request, `0` for a purely
    /// local change such as selecting a control.
    Admitted { request_id: i32 },
    /// The action did not apply; nothing was attempted.
    NotApplicable(ActionUnavailable),
    /// The existing admission refused it (full queue, resync, refused slot, ...);
    /// nothing was published and nothing is retried.
    Rejected(InventoryGestureError),
}

/// The facts one evaluation reads, gathered once.
struct Situation<'a> {
    hit: InventoryCellHit,
    role: TargetRole,
    cursor: Option<&'a NetworkItemStack>,
    target: Option<&'a NetworkItemStack>,
    crafter_toggle: bool,
    creative: bool,
    text_focused: bool,
    crafting_output: bool,
}

impl Situation<'_> {
    /// Whether gather may start from this cell, as the pointer's double press does.
    const fn gatherable(&self) -> bool {
        matches!(
            self.hit,
            InventoryCellHit::Player(_) | InventoryCellHit::Storage(_) | InventoryCellHit::Craft(_)
        ) && matches!(self.role, TargetRole::Item)
    }

    fn evaluate(
        &self,
        action: ContainerControllerAction,
    ) -> Result<ActionIntent, ActionUnavailable> {
        use ActionUnavailable as No;
        use ContainerControllerAction as A;
        match action {
            A::Primary => match self.role {
                TargetRole::Control | TargetRole::CreativeCatalog | TargetRole::RecipeBook => {
                    Ok(ActionIntent::Activate)
                }
                TargetRole::CraftOutput => Ok(if self.crafting_output {
                    ActionIntent::CraftOnce
                } else {
                    ActionIntent::TakeOutput
                }),
                TargetRole::StationOutput if self.target.is_some() => Ok(ActionIntent::TakeOutput),
                TargetRole::StationOutput => Err(No::OutputOnly),
                TargetRole::Item if self.crafter_toggle => Ok(ActionIntent::ToggleCrafterSlot),
                TargetRole::Item => match (self.cursor, self.target) {
                    (Some(_), Some(_)) => Ok(ActionIntent::Move),
                    (Some(_), None) => Ok(ActionIntent::Place),
                    (None, Some(_)) => Ok(ActionIntent::Take),
                    (None, None) => Err(No::NothingToAct),
                },
            },
            A::HalfOrPlaceOne => match self.role {
                TargetRole::Control => Err(No::NotApplicable),
                TargetRole::CraftOutput
                | TargetRole::StationOutput
                | TargetRole::CreativeCatalog
                | TargetRole::RecipeBook => Err(No::UnresolvedMapping),
                TargetRole::Item => match (self.cursor, self.target) {
                    (Some(_), _) => Ok(ActionIntent::PlaceOne),
                    (None, Some(_)) => Ok(ActionIntent::TakeHalf),
                    (None, None) => Err(No::NothingToAct),
                },
            },
            A::QuickTransfer => match self.role {
                TargetRole::Control => Err(No::NotApplicable),
                TargetRole::CraftOutput => Ok(if self.crafting_output {
                    ActionIntent::CraftAll
                } else {
                    ActionIntent::QuickTransfer
                }),
                TargetRole::CreativeCatalog | TargetRole::RecipeBook => {
                    Ok(ActionIntent::QuickTransfer)
                }
                TargetRole::Item | TargetRole::StationOutput => {
                    if self.target.is_some() {
                        Ok(ActionIntent::QuickTransfer)
                    } else {
                        Err(No::NothingToAct)
                    }
                }
            },
            A::Gather => match (self.gatherable(), self.cursor) {
                (false, _) => Err(No::NotApplicable),
                (true, None) => Err(No::CursorEmpty),
                (true, Some(_)) => Ok(ActionIntent::Gather),
            },
            A::DropOne | A::DropAll => {
                let intent = if action == A::DropOne {
                    ActionIntent::DropOne
                } else {
                    ActionIntent::DropAll
                };
                if self.text_focused {
                    Err(No::TextEntry)
                } else if self.cursor.is_some() {
                    Ok(intent)
                } else {
                    match self.role {
                        TargetRole::Item if self.target.is_some() => Ok(intent),
                        TargetRole::Item => Err(No::NothingToAct),
                        TargetRole::CreativeCatalog if self.creative => Ok(intent),
                        TargetRole::CraftOutput | TargetRole::StationOutput => {
                            Err(No::UnresolvedMapping)
                        }
                        _ => Err(No::NotApplicable),
                    }
                }
            }
        }
    }
}

impl UiRuntime {
    /// Captures the open container screen for later dispatch; `None` unless the UI is open.
    /// `screen_generation` is the coordinator's counter for this opening of the screen.
    pub fn container_screen_token(
        &self,
        player_runtime: &player_state::PlayerState,
        screen_generation: u64,
    ) -> Option<ScreenToken> {
        let ledger = self.inventory_ledger(player_runtime);
        self.inventory_open().then(|| ScreenToken {
            screen_generation,
            window: WindowStamp {
                kind: ledger.window_kind(),
                position: ledger.window_position(),
                generation: ledger.storage_generation(),
                actor: ledger.window_actor(),
            },
        })
    }

    /// Every action's availability for `hit` against the current ledger.
    pub fn container_controller_capabilities(
        &self,
        player_runtime: &player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Vec<ActionCapability> {
        let screen_open = self.inventory_open();
        let situation = self.situation(player_runtime, hit);
        ContainerControllerAction::ALL
            .into_iter()
            .map(|action| ActionCapability {
                action,
                availability: if screen_open {
                    situation.evaluate(action)
                } else {
                    Err(ActionUnavailable::ScreenClosed)
                },
                trigger: Trigger::Press,
                admission: Admission::AtDispatch,
            })
            .collect()
    }

    /// The prompts for `hit`: exactly the available capabilities, so a hint never offers
    /// an action that dispatch would refuse as not applicable.
    pub fn container_controller_hints(
        &self,
        player_runtime: &player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Vec<ActionHint> {
        self.container_controller_capabilities(player_runtime, hit)
            .into_iter()
            .filter_map(|capability| {
                Some(ActionHint {
                    action: capability.action,
                    intent: capability.availability.ok()?,
                })
            })
            .collect()
    }

    /// Runs one semantic action on `hit` through the existing inventory paths.
    ///
    /// `screen` and `resolved` were captured at the input edge. The coordinator must resolve
    /// `current` again against the current layout and semantic identities before dispatch.
    /// Replaced rows, reflow and window changes invalidate the action before ledger admission.
    pub fn dispatch_container_controller_action(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        action: ContainerControllerAction,
        screen: ScreenToken,
        resolved: &Selection,
        current: &Selection,
    ) -> ControllerActionOutcome {
        let Some(current_screen) = self.container_screen_token(player_runtime, current.generation)
        else {
            return ControllerActionOutcome::NotApplicable(ActionUnavailable::ScreenClosed);
        };
        if screen != current_screen {
            return ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleWindow);
        }
        if resolved != current {
            return ControllerActionOutcome::NotApplicable(ActionUnavailable::StaleTarget);
        }
        let hit = current.target.hit;
        if let Err(reason) = self.situation(player_runtime, hit).evaluate(action) {
            return ControllerActionOutcome::NotApplicable(reason);
        }
        match self.run_action(player_runtime, hit, action) {
            Some(Ok(request_id)) => ControllerActionOutcome::Admitted { request_id },
            Some(Err(error)) => ControllerActionOutcome::Rejected(error),
            None => ControllerActionOutcome::NotApplicable(ActionUnavailable::NotApplicable),
        }
    }

    fn situation<'a>(
        &self,
        player_runtime: &'a player_state::PlayerState,
        hit: InventoryCellHit,
    ) -> Situation<'a> {
        let ledger = self.inventory_ledger(player_runtime);
        let kind = ledger.window_kind();
        let role = match hit {
            InventoryCellHit::Storage(slot)
                if screen_lists(kind, slot, |name| name.ends_with(OUTPUT_COLLECTION_SUFFIX)) =>
            {
                TargetRole::StationOutput
            }
            InventoryCellHit::Player(_)
            | InventoryCellHit::Storage(_)
            | InventoryCellHit::Armor(_)
            | InventoryCellHit::Offhand
            | InventoryCellHit::Craft(_) => TargetRole::Item,
            InventoryCellHit::CraftOutput => TargetRole::CraftOutput,
            InventoryCellHit::CreativeGrid(_) => TargetRole::CreativeCatalog,
            InventoryCellHit::RecipeBook(_) => TargetRole::RecipeBook,
            InventoryCellHit::Widget(_)
            | InventoryCellHit::CreativeTab(_)
            | InventoryCellHit::CreativeSearch => TargetRole::Control,
        };
        let target = gesture_target(hit).and_then(|target| ledger.target_stack(target));
        let cursor = ledger.cursor_stack();
        Situation {
            hit,
            role,
            cursor,
            target,
            crafter_toggle: kind == Some(WindowKind::Crafter)
                && cursor.is_none()
                && target.is_none()
                && matches!(
                    hit,
                    InventoryCellHit::Storage(slot)
                        if screen_lists(kind, slot, |_| true)
                            && !self.screen_state().crafter.is_disabled(slot)
                ),
            creative: player_runtime.facts.player_game_mode()
                == Some(protocol::PlayerGameMode::Creative),
            text_focused: self.screen_state().text_focused(),
            crafting_output: matches!(kind, None | Some(WindowKind::Workbench)),
        }
    }

    /// Performs an evaluated action; `None` when the existing owners have no route for it.
    fn run_action(
        &mut self,
        player_runtime: &mut player_state::PlayerState,
        hit: InventoryCellHit,
        action: ContainerControllerAction,
    ) -> Option<Result<i32, InventoryGestureError>> {
        let pointer = match action {
            ContainerControllerAction::Primary => PointerAction::Click(hit),
            ContainerControllerAction::HalfOrPlaceOne => PointerAction::SecondaryClick(hit),
            ContainerControllerAction::QuickTransfer => PointerAction::QuickMove(hit),
            ContainerControllerAction::Gather => PointerAction::Gather,
            ContainerControllerAction::DropOne | ContainerControllerAction::DropAll => {
                let amount = (action == ContainerControllerAction::DropOne).then_some(1);
                if self
                    .inventory_ledger(player_runtime)
                    .cursor_stack()
                    .is_some()
                {
                    return Some(
                        self.inventory_ledger_mut(player_runtime)
                            .begin_drop(DropSource::Cursor, amount),
                    );
                }
                if let Some(outcome) = self.drop_creative_hit(player_runtime, hit, amount.is_none())
                {
                    return Some(outcome);
                }
                let target = gesture_target(hit)?;
                return Some(
                    self.inventory_ledger_mut(player_runtime)
                        .begin_drop(DropSource::Target(target), amount),
                );
            }
        };
        Some(self.run_pointer_action(player_runtime, pointer))
    }
}

#[cfg(test)]
#[path = "actions/tests.rs"]
mod tests;
