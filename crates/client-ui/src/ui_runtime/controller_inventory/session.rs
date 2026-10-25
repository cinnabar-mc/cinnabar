//! Retained input arbitration for container cells and recipe-panel controls.
//!
//! The caller owns modal/text arbitration and scroll reveal. Ordinary inventory cells,
//! recipe-book rows and the recipe panel's controls (category tabs, filter, layout and
//! right-tab radios, furnace tabs, book paging) are admitted. Other widgets, search
//! fields and the fallback creative grid stay excluded until their authored activation
//! mappings are integrated. Recipe rows are identified by the content key the caller
//! supplies, so a filtered row never inherits focus or an action from its old position.
//! No pointer clicks are synthesized.

use super::{actions::ContainerControllerAction, navigation};
use crate::ui_runtime::UiRuntime;
use ui_screens::container_actions::{InventoryCellHit, Widget};

/// Held logical buttons, in `ContainerControllerAction::ALL` order.
pub struct Input {
    pub navigation: navigation::Input,
    pub actions: [bool; ContainerControllerAction::ALL.len()],
}

/// Current geometry selection and press-only edges; dispatch still revalidates authority.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub navigation: navigation::Output,
    pub actions: Vec<ContainerControllerAction>,
}

/// What a target's identity is interned from: the cell itself, or a list row's content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Semantic {
    Cell(InventoryCellHit),
    Row(u64),
}

/// Retain in the app resource. Reset on any higher-priority input owner.
pub struct ControllerSession {
    navigation: navigation::Navigation,
    owner: Option<(u64, u64)>,
    armed: bool,
    held: [bool; ContainerControllerAction::ALL.len()],
    previous_time: Option<std::time::Duration>,
    deadzone: f64,
    identities: Vec<Semantic>,
}

impl ControllerSession {
    /// Constructs a session using explicit, provisional cursor/repeat policy.
    pub fn new(settings: navigation::Settings) -> Result<Self, &'static str> {
        Ok(Self {
            navigation: navigation::Navigation::new(settings)?,
            owner: None,
            armed: false,
            held: [false; ContainerControllerAction::ALL.len()],
            previous_time: None,
            deadzone: settings.cursor_deadzone,
            identities: Vec::new(),
        })
    }

    /// Cancels local gestures; no held action survives until a full neutral sample.
    pub fn reset(&mut self) {
        self.navigation.reset();
        self.owner = None;
        self.armed = false;
        self.held.fill(false);
        self.previous_time = None;
        self.identities.clear();
    }

    /// Interns admitted semantic targets, preserving identity across layout reflow.
    /// Repeated regions for one cell or recipe content use the first association in
    /// layout order. A supplied `identity` is the content key for recipe rows and is
    /// ignored for every other cell.
    pub fn targets(
        &mut self,
        associations: impl IntoIterator<Item = navigation::Target>,
    ) -> Vec<navigation::Target> {
        let mut targets: Vec<navigation::Target> = Vec::new();
        let mut semantics: Vec<Semantic> = Vec::new();
        for association in associations {
            let hit = association.hit;
            if !admitted_cell(hit) {
                continue;
            }
            let semantic = if matches!(hit, InventoryCellHit::RecipeBook(_)) {
                Semantic::Row(association.identity)
            } else {
                Semantic::Cell(hit)
            };
            if semantics.contains(&semantic)
                || targets
                    .iter()
                    .any(|target| target.hit == hit || target.key == association.key)
            {
                continue;
            }
            let identity = match self.identities.iter().position(|cell| *cell == semantic) {
                Some(index) => index,
                None => {
                    self.identities.push(semantic);
                    self.identities.len() - 1
                }
            };
            semantics.push(semantic);
            targets.push(navigation::Target {
                key: association.key,
                identity: u64::try_from(identity).expect("cell identity exceeds u64"),
                hit,
            });
        }
        targets
    }

    /// Reduces one current layout and publishes its pointer/focus before snapshot.
    /// `active` must exclude text fields, modals, disconnect and window focus loss.
    pub fn step(
        &mut self,
        runtime: &mut UiRuntime,
        frame: navigation::Frame<'_>,
        mut input: Input,
    ) -> Output {
        let owner = (frame.generation, input.navigation.controller);
        if !runtime.inventory_open()
            || runtime.screen_state().text_focused()
            || frame.generation != runtime.container_generation()
            || !input.navigation.active
        {
            self.reset();
            runtime.set_inventory_controller_focus(None);
            runtime.set_inventory_pointer_gui(None);
            runtime.set_inventory_controller_cursor_visible(false);
            return Output::default();
        }
        if self.owner != Some(owner)
            || self
                .previous_time
                .is_some_and(|time| time > input.navigation.now)
        {
            self.reset();
            self.owner = Some(owner);
        }
        self.previous_time = Some(input.navigation.now);
        let mouse = input.navigation.mouse.is_some();
        let axes = input
            .navigation
            .axes
            .map(|axis| if axis.is_finite() { axis } else { 0.0 });
        let neutral = !input.actions.iter().any(|held| *held)
            && !input.navigation.directions.iter().any(|held| *held)
            && axes[0].hypot(axes[1]) <= self.deadzone;
        if mouse {
            self.armed = false;
        }
        let was_armed = self.armed;
        if !self.armed && neutral && !mouse {
            self.armed = true;
        }
        // Navigation has its own neutral gate; action-held opens must also keep
        // that gate closed, rather than silently allowing focus underneath them.
        if !was_armed && !neutral && !mouse {
            self.navigation.reset();
            input.navigation.directions = [false; 4];
            input.navigation.axes = [0.0; 2];
        }
        let focusable: std::collections::HashSet<_> = frame
            .hits
            .iter()
            .filter(|hit| hit.takes_focus())
            .map(|hit| hit.key.as_str())
            .collect();
        let mut targets = self.targets(
            frame
                .targets
                .iter()
                .filter(|target| focusable.contains(target.key.as_str()))
                .cloned(),
        );
        let aliases: Vec<_> = frame
            .targets
            .iter()
            .filter_map(|alias| {
                let cell = targets.iter().find(|target| target.hit == alias.hit)?;
                (cell.key != alias.key).then(|| navigation::Target {
                    key: alias.key.clone(),
                    ..cell.clone()
                })
            })
            .collect();
        // Preserve authored routing and modal metadata, but do not let controls
        // outside this slice trap directional navigation between ordinary cells.
        let hits: Vec<_> = frame
            .hits
            .iter()
            .cloned()
            .map(|mut hit| {
                if !targets.iter().any(|target| target.key == hit.key)
                    && let Some(focus) = &mut hit.focus
                {
                    focus.enabled = false;
                }
                hit
            })
            .collect();
        targets.extend(aliases);
        let mut navigation = self
            .navigation
            .update(
                navigation::Frame {
                    targets: &targets,
                    hits: &hits,
                    ..frame
                },
                input.navigation,
            )
            .clone();
        let actions: Vec<_> = ContainerControllerAction::ALL
            .into_iter()
            .enumerate()
            .filter(|(index, _)| {
                was_armed
                    && !mouse
                    && input.actions[*index]
                    && !self.held[*index]
                    && navigation.selection.is_some()
            })
            .map(|(_, action)| action)
            .collect();
        if !actions.is_empty() {
            navigation = self.navigation.acquire_cursor().clone();
        }
        self.held = input.actions;
        runtime.set_inventory_pointer_gui(
            navigation.cursor.map(|point| point.map(|axis| axis as f32)),
        );
        runtime.set_inventory_controller_cursor_visible(
            matches!(
                navigation.mode,
                navigation::Mode::Focus | navigation::Mode::Cursor
            ) && navigation.cursor.is_some(),
        );
        runtime.set_inventory_controller_focus(
            (navigation.mode == navigation::Mode::Focus)
                .then(|| navigation.focused_key.clone())
                .flatten(),
        );
        Output {
            navigation,
            actions,
        }
    }
}

/// Cells with an existing pointer route the controller may drive: inventory cells, recipe
/// rows and the recipe panel's controls. Other widgets wait for their authored mappings.
fn admitted_cell(hit: InventoryCellHit) -> bool {
    matches!(
        hit,
        InventoryCellHit::Player(_)
            | InventoryCellHit::Storage(_)
            | InventoryCellHit::Armor(_)
            | InventoryCellHit::Offhand
            | InventoryCellHit::Craft(_)
            | InventoryCellHit::CraftOutput
            | InventoryCellHit::RecipeBook(_)
            | InventoryCellHit::CreativeTab(_)
            | InventoryCellHit::Widget(
                Widget::RecipeFilter
                    | Widget::InventoryLayout(_)
                    | Widget::InventoryRightTab(_)
                    | Widget::FurnaceTab(_)
                    | Widget::FurnaceClearRecipe
                    | Widget::BookToggle
                    | Widget::BookRecipe(_)
                    | Widget::BookPage { .. }
            )
    )
}

/// Content keys of the recipe book's rows, indexed like `InventoryCellHit::RecipeBook`.
pub fn recipe_row_identities(
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
) -> Vec<u64> {
    crate::ui_runtime::inventory_actions::recipe_book_entries(player_runtime, runtime)
        .iter()
        .map(|entry| entry.identity())
        .collect()
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
