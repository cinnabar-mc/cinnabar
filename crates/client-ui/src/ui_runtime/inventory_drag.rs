//! Pointer gesture state for the inventory screens: drag-distribute and
//! double-click gather. Pure so the click timing is testable.

use inventory::inventory_ledger::{DragDistribution, MAX_DISTRIBUTION_CELLS};
use protocol::NetworkItemStack;
use ui_screens::container_actions::InventoryCellHit;

/// Two primary presses this close on one cell gather the held item.
pub const DOUBLE_CLICK_MILLIS: u64 = 250;
/// Bounds a drag's remembered cells.
const MAX_DRAG_CELLS: usize = MAX_DISTRIBUTION_CELLS;

/// What one frame's pointer input asks the inventory to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PointerAction {
    /// A plain primary click on a cell.
    Click(InventoryCellHit),
    /// A plain secondary click on a cell.
    SecondaryClick(InventoryCellHit),
    /// A primary or secondary click with Shift held.
    QuickMove(InventoryCellHit),
    /// A drag over two or more cells with the held stack.
    Distribute {
        cells: Vec<InventoryCellHit>,
        one_each: bool,
    },
    /// Clears incremental split accounting after the matching button is released.
    EndDistribute,
    /// Collects every compatible stack; Shift routes an open chest's stacks into storage.
    Gather {
        hit: InventoryCellHit,
        shift: bool,
        /// The stack that was under the pointer before the first click.
        focus: Option<NetworkItemStack>,
    },
}

/// One frame of button edges plus the cell under the pointer.
#[derive(Debug, Clone)]
pub struct PointerFrame {
    pub primary_pressed: bool,
    pub primary_released: bool,
    pub secondary_pressed: bool,
    pub secondary_released: bool,
    pub shift: bool,
    /// Whether the cursor holds an item.
    pub holding: bool,
    pub hit: Option<InventoryCellHit>,
    /// The stack under `hit` before this frame's click is applied.
    pub hit_stack: Option<NetworkItemStack>,
    pub now_millis: u64,
}

#[derive(Debug, Clone, Default)]
struct Drag {
    secondary: bool,
    cells: Vec<InventoryCellHit>,
}

#[derive(Debug, Clone, Default)]
pub struct InventoryPointer {
    drag: Option<Drag>,
    last_primary: Option<(InventoryCellHit, u64, bool, Option<NetworkItemStack>)>,
    pub(crate) distribution: Option<DragDistribution>,
}

fn draggable(hit: InventoryCellHit) -> bool {
    matches!(
        hit,
        InventoryCellHit::Player(_) | InventoryCellHit::Storage(_) | InventoryCellHit::Craft(_)
    )
}

impl InventoryPointer {
    /// Forgets any drag in flight, e.g. when the screen changes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advances the gesture state by one frame and returns what to dispatch.
    pub fn step(&mut self, frame: PointerFrame) -> Vec<PointerAction> {
        let mut actions = Vec::new();
        // A fresh press starts a new gesture, not another slot in the old drag.
        if frame.primary_pressed || frame.secondary_pressed {
            self.drag = None;
            self.distribution = None;
        }
        if let (Some(drag), Some(hit)) = (self.drag.as_mut(), frame.hit)
            && draggable(hit)
            && !drag.cells.contains(&hit)
            && drag.cells.len() < MAX_DRAG_CELLS
        {
            drag.cells.push(hit);
            actions.push(PointerAction::Distribute {
                cells: drag.cells.clone(),
                one_each: drag.secondary,
            });
        }
        if frame.primary_released || frame.secondary_released {
            // The press already acted on the first cell; release only ends a split.
            if let Some(drag) = self.drag.take() {
                let released = if drag.secondary {
                    frame.secondary_released
                } else {
                    frame.primary_released
                };
                if released {
                    if drag.cells.len() >= 2 {
                        actions.push(PointerAction::EndDistribute);
                    }
                } else {
                    self.drag = Some(drag);
                }
            }
        }
        if frame.primary_pressed {
            self.press_primary(&frame, &mut actions);
        }
        // When both edges arrive together the primary operation wins.
        if frame.secondary_pressed && !frame.primary_pressed {
            self.drag = None;
            // A secondary click is an intervening gesture, so the next primary
            // press must not be mistaken for the second half of a double-click.
            self.last_primary = None;
            let Some(hit) = frame.hit else {
                return actions;
            };
            if frame.shift {
                self.last_primary = None;
                actions.push(PointerAction::QuickMove(hit));
                return actions;
            }
            if frame.holding && draggable(hit) {
                self.distribution = None;
                self.drag = Some(Drag {
                    secondary: true,
                    cells: vec![hit],
                });
            }
            actions.push(PointerAction::SecondaryClick(hit));
        }
        actions
    }

    fn press_primary(&mut self, frame: &PointerFrame, actions: &mut Vec<PointerAction>) {
        self.drag = None;
        let Some(hit) = frame.hit else {
            // Clicking outside the inventory also breaks a double-click pair.
            self.last_primary = None;
            return;
        };
        let double = self
            .last_primary
            .as_ref()
            .is_some_and(|(cell, at, was_shift, _)| {
                *cell == hit
                    && *was_shift == frame.shift
                    && frame.now_millis.saturating_sub(*at) <= DOUBLE_CLICK_MILLIS
            });
        let focus = self
            .last_primary
            .as_ref()
            .and_then(|(_, _, _, stack)| stack.clone())
            .or_else(|| frame.hit_stack.clone());
        self.last_primary = Some((hit, frame.now_millis, frame.shift, frame.hit_stack.clone()));
        if double && frame.holding && draggable(hit) {
            self.last_primary = None;
            actions.push(PointerAction::Gather {
                hit,
                shift: frame.shift,
                focus,
            });
        } else if frame.shift {
            // Keep this press as a possible second Shift-click. If the cursor
            // is already holding an item, a matching second press becomes the
            // chest transfer gesture above.
            actions.push(PointerAction::QuickMove(hit));
        } else {
            // Vanilla places on the press; visiting a second cell turns it into a live split.
            if frame.holding && draggable(hit) {
                self.distribution = None;
                self.drag = Some(Drag {
                    secondary: false,
                    cells: vec![hit],
                });
            }
            actions.push(PointerAction::Click(hit));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: InventoryCellHit = InventoryCellHit::Player(9);
    const B: InventoryCellHit = InventoryCellHit::Player(10);

    fn frame(hit: Option<InventoryCellHit>, now: u64) -> PointerFrame {
        PointerFrame {
            primary_pressed: false,
            primary_released: false,
            secondary_pressed: false,
            secondary_released: false,
            shift: false,
            holding: true,
            hit,
            hit_stack: None,
            now_millis: now,
        }
    }

    /// A held-stack press places in its own frame; the release adds nothing.
    #[test]
    fn held_stack_press_clicks_before_release() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            let press = PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                ..frame(Some(A), 0)
            };
            assert_eq!(
                pointer.step(press),
                vec![if secondary {
                    PointerAction::SecondaryClick(A)
                } else {
                    PointerAction::Click(A)
                }]
            );
            let release = PointerFrame {
                primary_released: !secondary,
                secondary_released: secondary,
                ..frame(Some(A), 10)
            };
            assert!(pointer.step(release).is_empty());
        }
    }

    #[test]
    fn dragging_over_two_cells_distributes() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 0)
        });
        let actions = pointer.step(frame(Some(B), 5));
        assert_eq!(
            actions,
            vec![PointerAction::Distribute {
                cells: vec![A, B],
                one_each: false,
            }]
        );
        let actions = pointer.step(PointerFrame {
            primary_released: true,
            ..frame(Some(B), 10)
        });
        assert_eq!(actions, vec![PointerAction::EndDistribute]);
    }

    #[test]
    fn secondary_drag_places_one_each() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            secondary_pressed: true,
            ..frame(Some(A), 0)
        });
        let actions = pointer.step(frame(Some(B), 5));
        assert_eq!(
            actions,
            vec![PointerAction::Distribute {
                cells: vec![A, B],
                one_each: true
            }]
        );
        assert_eq!(
            pointer.step(PointerFrame {
                secondary_released: true,
                ..frame(Some(B), 10)
            }),
            vec![PointerAction::EndDistribute]
        );
    }

    #[test]
    fn releasing_both_buttons_ends_the_matching_drag_before_later_hover() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            pointer.step(PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                ..frame(Some(A), 0)
            });
            pointer.step(frame(Some(B), 5));
            assert_eq!(
                pointer.step(PointerFrame {
                    primary_released: true,
                    secondary_released: true,
                    ..frame(Some(B), 10)
                }),
                vec![PointerAction::EndDistribute]
            );
            assert!(
                pointer
                    .step(frame(Some(InventoryCellHit::Player(11)), 15))
                    .is_empty()
            );
        }
    }

    #[test]
    fn releasing_the_other_button_preserves_the_active_drag() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            pointer.step(PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                ..frame(Some(A), 0)
            });
            assert!(
                pointer
                    .step(PointerFrame {
                        primary_released: secondary,
                        secondary_released: !secondary,
                        ..frame(Some(A), 5)
                    })
                    .is_empty()
            );
            assert_eq!(
                pointer.step(frame(Some(B), 10)),
                vec![PointerAction::Distribute {
                    cells: vec![A, B],
                    one_each: secondary
                }]
            );
        }
    }

    #[test]
    fn live_drag_continues_after_the_cursor_becomes_empty() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 0)
        });
        pointer.step(frame(Some(B), 5));
        let c = InventoryCellHit::Player(11);
        assert_eq!(
            pointer.step(PointerFrame {
                holding: false,
                ..frame(Some(c), 8)
            }),
            vec![PointerAction::Distribute {
                cells: vec![A, B, c],
                one_each: false
            }]
        );
        assert!(
            pointer
                .step(PointerFrame {
                    holding: false,
                    ..frame(Some(A), 9)
                })
                .is_empty()
        );
        assert_eq!(
            pointer.step(PointerFrame {
                primary_released: true,
                holding: false,
                ..frame(Some(c), 10)
            }),
            vec![PointerAction::EndDistribute]
        );
    }

    #[test]
    fn quick_second_press_gathers() {
        let mut pointer = InventoryPointer::default();
        let empty_handed = PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        };
        assert_eq!(pointer.step(empty_handed), vec![PointerAction::Click(A)]);
        let second = PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 100)
        };
        assert_eq!(
            pointer.step(second),
            vec![PointerAction::Gather {
                hit: A,
                shift: false,
                focus: None,
            }]
        );
        let late = PointerFrame {
            primary_pressed: true,
            ..frame(Some(A), 900)
        };
        assert_eq!(pointer.step(late), vec![PointerAction::Click(A)]);
    }

    #[test]
    fn shift_double_click_gathers_while_holding_an_item() {
        let mut pointer = InventoryPointer::default();
        let focus = NetworkItemStack {
            network_id: 6,
            count: 20,
            ..NetworkItemStack::default()
        };
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                shift: true,
                hit_stack: Some(focus.clone()),
                ..frame(Some(A), 0)
            }),
            vec![PointerAction::QuickMove(A)]
        );
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                shift: true,
                ..frame(Some(A), 100)
            }),
            vec![PointerAction::Gather {
                hit: A,
                shift: true,
                focus: Some(focus),
            }]
        );
    }

    #[test]
    fn an_intervening_secondary_press_breaks_gather() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        });
        assert_eq!(
            pointer.step(PointerFrame {
                secondary_pressed: true,
                ..frame(Some(A), 50)
            }),
            vec![PointerAction::SecondaryClick(A)]
        );
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                ..frame(Some(A), 100)
            }),
            vec![PointerAction::Click(A)]
        );
    }

    #[test]
    fn clicking_outside_the_inventory_breaks_gather() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        });
        assert!(
            pointer
                .step(PointerFrame {
                    primary_pressed: true,
                    ..frame(None, 50)
                })
                .is_empty()
        );
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                ..frame(Some(A), 100)
            }),
            vec![PointerAction::Click(A)]
        );
    }

    #[test]
    fn shift_click_quick_moves_with_either_button() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            let press = PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                shift: true,
                holding: false,
                ..frame(Some(A), 0)
            };
            assert_eq!(pointer.step(press), vec![PointerAction::QuickMove(A)]);
        }
    }
    #[test]
    fn shift_quick_move_wins_over_a_recent_primary_click() {
        let mut pointer = InventoryPointer::default();
        pointer.step(PointerFrame {
            primary_pressed: true,
            holding: false,
            ..frame(Some(A), 0)
        });
        assert_eq!(
            pointer.step(PointerFrame {
                primary_pressed: true,
                shift: true,
                ..frame(Some(A), 10)
            }),
            vec![PointerAction::QuickMove(A)]
        );
    }

    #[test]
    fn a_new_completed_gesture_discards_the_previous_drag() {
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            pointer.step(PointerFrame {
                primary_pressed: true,
                ..frame(Some(A), 0)
            });
            let actions = pointer.step(PointerFrame {
                primary_pressed: !secondary,
                secondary_pressed: secondary,
                shift: !secondary,
                holding: false,
                ..frame(Some(B), 500)
            });
            assert_eq!(
                actions,
                vec![if secondary {
                    PointerAction::SecondaryClick(B)
                } else {
                    PointerAction::QuickMove(B)
                }]
            );
            assert!(
                pointer
                    .step(PointerFrame {
                        primary_released: true,
                        ..frame(Some(B), 510)
                    })
                    .is_empty()
            );
        }
    }

    #[test]
    fn a_new_drag_does_not_distribute_into_the_previous_drag() {
        let c = InventoryCellHit::Player(11);
        for secondary in [false, true] {
            let mut pointer = InventoryPointer::default();
            pointer.step(PointerFrame {
                primary_pressed: true,
                ..frame(Some(A), 0)
            });
            assert_eq!(
                pointer.step(PointerFrame {
                    primary_pressed: !secondary,
                    secondary_pressed: secondary,
                    ..frame(Some(B), 500)
                }),
                vec![if secondary {
                    PointerAction::SecondaryClick(B)
                } else {
                    PointerAction::Click(B)
                }]
            );
            assert_eq!(
                pointer.step(frame(Some(c), 505)),
                vec![PointerAction::Distribute {
                    cells: vec![B, c],
                    one_each: secondary,
                }]
            );
            assert_eq!(
                pointer.step(PointerFrame {
                    primary_released: !secondary,
                    secondary_released: secondary,
                    ..frame(Some(c), 510)
                }),
                vec![PointerAction::EndDistribute]
            );
        }
    }
}
