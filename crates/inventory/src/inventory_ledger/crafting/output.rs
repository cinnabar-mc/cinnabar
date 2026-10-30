//! Shared result declaration, consumption and placement for ordinary and special crafts.
use super::*;
use protocol::NetworkItemStack;

/// A validated craft's concrete result and current ingredient cell identities.
pub(super) struct CraftSubmissionPlan<'a> {
    pub id: u32,
    pub crafts: u8,
    pub created: NetworkItemStack,
    pub results: Arc<[CraftResult]>,
    pub consumed: Vec<(Cell, u8, i32)>,
    pub extra: Option<&'a RecipeHandle>,
}

impl PlayerInventoryLedger {
    /// Places a concrete craft through the same surface, identity and capacity gates for every recipe.
    pub(super) fn finish_craft_submission(
        &self,
        plan: CraftSubmissionPlan<'_>,
        sink: CraftSink,
    ) -> Result<Submission, InventoryGestureError> {
        let personal_generation = self.gesture_preflight(true)?;
        self.check_surfaces([Cell::Cursor, Cell::CreatedOutput])?;
        let destination = match sink {
            CraftSink::Cursor => Some(Cell::Cursor),
            CraftSink::Player(slot) => {
                let cell = Cell::Inventory(slot);
                self.check_target(cell)?;
                self.check_surfaces([cell])?;
                Some(cell)
            }
            CraftSink::Inventory => None,
        };
        let CraftSubmissionPlan {
            id,
            crafts,
            created,
            results,
            consumed,
            extra,
        } = plan;
        let request_id = created.stack_network_id;
        let capacity = self
            .negotiated_item_entry(created.network_id)
            .and_then(entry_capacity)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let total = u8::try_from(created.count)
            .ok()
            .filter(|count| *count > 0 && *count <= capacity)
            .ok_or(InventoryGestureError::InvalidRequest)?;
        let mut actions = vec![
            StackRequestAction::CraftRecipe {
                recipe_network_id: id,
                crafts,
            },
            // One craft's results, every one of them, under a single times-crafted count.
            StackRequestAction::CraftResultsDeprecated { results, crafts: 1 },
        ];
        let mut groups = Vec::new();
        // Cells this request consumes from never take a result; the planner tracks the rest.
        let mut closed = Vec::new();
        for (cell, total, id) in consumed {
            for amount in action_amounts(u16::from(total)) {
                actions.push(StackRequestAction::Consume {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: helpers::request_slot(cell, id, None)?,
                });
                groups.push(DeltaGroup::Shrink {
                    source: cell,
                    amount,
                    source_id: id,
                });
            }
            closed.push(cell);
        }
        let extras = extra.is_some_and(|recipe| recipe.extra_outputs().len() != 0);
        if extras {
            // With several results the main one is created by index too.
            actions.push(StackRequestAction::Create { results_index: 0 });
        }
        let output_slot = helpers::request_slot(Cell::CreatedOutput, request_id, None)?;
        groups.push(DeltaGroup::Set {
            cell: Cell::CreatedOutput,
            held: Held {
                source_stack_id: Some(created.stack_network_id),
                source_origin: None,
                stack: created.clone(),
                overlay: None,
                acknowledged_request_id: None,
            },
        });
        let mut registry_bound_merge = false;
        match destination {
            // A single cell takes the whole output in one action.
            Some(_) if u16::from(total) > placement::action_amount_limit(u16::MAX) => {
                return Err(InventoryGestureError::InvalidRequest);
            }
            Some(cell) => {
                let into = self.view().get(cell);
                if let Some(into) = into {
                    if self.awaiting_identity(into) {
                        return Err(InventoryGestureError::AwaitingIdentity);
                    }
                    if !Self::same_item(&created, &into.stack)
                        || super::super::registry::held_item_data_equal(
                            &Held::new(&created).expect("created output is nonempty"),
                            into,
                        ) != Some(true)
                        || into.stack.count.saturating_add(u16::from(total)) > u16::from(capacity)
                    {
                        return Err(InventoryGestureError::InvalidRequest);
                    }
                    registry_bound_merge = true;
                }
                let destination_id = into.map(|held| held.stack.stack_network_id);
                let destination_slot =
                    helpers::request_slot(cell, destination_id.unwrap_or(0), None)?;
                actions.push(match sink {
                    CraftSink::Cursor => StackRequestAction::Take {
                        amount: total,
                        source: output_slot,
                        destination: destination_slot,
                    },
                    _ => StackRequestAction::Place {
                        amount: total,
                        source: output_slot,
                        destination: destination_slot,
                    },
                });
                groups.push(DeltaGroup::Transfer {
                    source: Cell::CreatedOutput,
                    destination: cell,
                    amount: u16::from(total),
                    source_id: request_id,
                    destination_id,
                    capacity: destination_id.map(|_| u16::from(capacity)),
                });
                closed.push(cell);
            }
            None => {}
        }
        let mut planner = OutputPlanner::new(closed);
        if destination.is_none() {
            let plan = planner.plan(self, &created, capacity, &self.player_cells(), false);
            let placed: u16 = plan.iter().map(|(_, amount, _)| *amount).sum();
            if placed != created.count {
                return Err(InventoryGestureError::InvalidRequest);
            }
            for (cell, amount, into) in plan {
                registry_bound_merge |= into.is_some();
                let Cell::Inventory(slot) = cell else {
                    return Err(InventoryGestureError::InvalidRequest);
                };
                // Results auto-placed into the inventory use its combined name.
                actions.push(StackRequestAction::Place {
                    amount: u8::try_from(amount)
                        .map_err(|_| InventoryGestureError::InvalidRequest)?,
                    source: output_slot,
                    destination: helpers::combined_request_slot(
                        slot,
                        into.map_or(0, |(id, _)| id),
                    )?,
                });
                groups.push(DeltaGroup::Transfer {
                    source: Cell::CreatedOutput,
                    destination: cell,
                    amount,
                    source_id: request_id,
                    destination_id: into.map(|(id, _)| id),
                    capacity: into.map(|_| u16::from(capacity)),
                });
            }
        }
        if extras {
            registry_bound_merge |= self.push_extra_outputs(
                extra.expect("extra outputs have a named recipe"),
                crafts,
                request_id,
                ExtraDestination::Grid,
                &mut planner,
                &mut actions,
                &mut groups,
            )?;
        }
        // An unencodable request never enters the queue; a craft-all scope shrinks instead.
        if actions.len() > protocol::MAX_STACK_REQUEST_ACTIONS {
            return Err(InventoryGestureError::InvalidRequest);
        }
        Ok(Submission {
            actions,
            groups,
            personal_generation,
            // The crafted stack must settle under a real server id.
            requires_distinct_stack_ids: true,
            registry_bound_merge,
        })
    }
}
