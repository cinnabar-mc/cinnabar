use super::*;

/// A reproducible stream of metadata, input and terrain choices.
struct Random(u64);

impl Random {
    /// Advances a fixed-seed generator without depending on a random library version.
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Static scaffold layers, with optional collision shapes around the teleport anchor.
struct ContactWorld {
    seed: u64,
    collision: bool,
}

impl ContactWorld {
    /// Selects scaffolding from the cell identity, independently of collision queries.
    fn scaffolding(&self, block: [i32; 3]) -> bool {
        let mut hash = self.seed;
        for coordinate in block {
            hash = hash
                .wrapping_mul(0x9e37_79b9)
                .wrapping_add(coordinate as u64);
        }
        hash.rotate_right(13) & 1 == 0
    }
}

impl sim::CollisionWorld for ContactWorld {
    /// Returns the same shapes as the provenance query.
    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        let colliders = self.collision_boxes_with_provenance(query)?;
        Ok(sim::CollisionQuery {
            value: colliders
                .value
                .into_iter()
                .map(|collider| collider.aabb)
                .collect(),
            identity: colliders.identity,
        })
    }

    /// Emits ordinary scaffold cubes so contextual anchor queries must filter them.
    fn collision_boxes_with_provenance(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::ProvenancedCollider>>, sim::WorldQueryError> {
        let mut colliders = Vec::new();
        if self.collision {
            for x in query.min.x.floor() as i32..=query.max.x.floor() as i32 {
                for y in query.min.y.floor() as i32..=query.max.y.floor() as i32 {
                    for z in query.min.z.floor() as i32..=query.max.z.floor() as i32 {
                        if self.scaffolding([x, y, z]) {
                            let min = sim::Vec3::new(f64::from(x), f64::from(y), f64::from(z));
                            colliders.push(sim::ProvenancedCollider {
                                aabb: sim::Aabb::new(min, min + sim::Vec3::ONE),
                                block: Some([x, y, z]),
                                runtime_id: None,
                                cell_has_scaffolding: Some(true),
                            });
                        }
                    }
                }
            }
        }
        Ok(sim::CollisionQuery::synthetic(colliders))
    }

    /// Selects scaffolding from the cell identity, independently of query order.
    fn block_physics(
        &self,
        block: [i32; 3],
    ) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: if self.scaffolding(block) {
                    sim::BlockPhysicsFlags::SCAFFOLDING
                } else {
                    sim::BlockPhysicsFlags::PASSABLE
                },
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// An accepted edit in the reference stream, retaining its arrival order.
#[derive(Clone, Copy)]
struct Edit {
    mask: u64,
    word: u64,
}

/// A completed live input and its immutable comparison capture.
#[derive(Clone)]
struct ModelTick {
    input: sim::MovementInput,
    entry: u64,
    edits: Vec<Edit>,
    /// Only the pose and spin bits changed by the accepted live write.
    server_flags: client_world::MovementFlagUpdate,
}

/// Reads only the locally owned climber flags, without consuming a sync countdown.
fn flags(physics: &LocalPhysicsController) -> u64 {
    physics.prediction_sync_flags().extended_flags.unwrap_or(0) & ClimberFlags::mask()
}

/// Applies reference edits as ordinary live writes before the next input.
fn apply_edits(physics: &mut LocalPhysicsController, edits: &[Edit]) {
    for edit in edits {
        let word = (flags(physics) & !edit.mask) | (edit.word & edit.mask);
        assert_eq!(
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    extended_flags: Some(word),
                    ..Default::default()
                },
            ),
            None
        );
    }
}

/// Rebuilds the reference with live ticks only; it never uses correction replay.
fn run_live_model(
    ticks: &[ModelTick],
    pending: &[Edit],
    world: &ContactWorld,
) -> (
    LocalPhysicsController,
    Vec<crate::movement::PhysicsMovementSample>,
) {
    let mut physics = due_physics();
    let mut samples = Vec::new();
    for tick in ticks {
        apply_edits(&mut physics, &tick.edits);
        let current = physics.state().unwrap().tick;
        assert_eq!(
            physics.apply_server_movement_flags(current, tick.server_flags),
            None
        );
        let frame = physics.advance(simulation_clock::TICK_DURATION, tick.input, world);
        assert_eq!(frame.completed_ticks, 1);
        samples.extend(frame.samples);
    }
    apply_edits(&mut physics, pending);
    (physics, samples)
}

/// Compares every replayed movement sample and the final flags with fresh live execution.
fn check_replay(
    physics: &mut LocalPhysicsController,
    anchor: u64,
    ticks: &[ModelTick],
    pending: &[Edit],
    world: &ContactWorld,
    seed: u64,
) {
    let (expected, samples) = run_live_model(ticks, pending, world);
    let replay = physics.replay_retained_from(anchor, world).unwrap();
    let expected_samples: Vec<_> = samples
        .iter()
        .filter(|sample| sample.tick > anchor)
        .collect();
    assert_eq!(replay.replayed_samples.len(), expected_samples.len());
    for (actual, expected) in replay.replayed_samples.iter().zip(expected_samples) {
        assert_eq!(
            actual.movement, expected.movement,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.position, expected.position,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.velocity, expected.velocity,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.processed, expected.processed,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.input.server_pose_flags, expected.input.server_pose_flags,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
        assert_eq!(
            actual.input.server_spinning, expected.input.server_spinning,
            "seed={seed} anchor={anchor} tick={}",
            actual.tick
        );
    }
    assert_eq!(
        flags(physics),
        flags(&expected),
        "seed={seed} anchor={anchor}"
    );
    assert_eq!(
        physics.pose_flags(),
        expected.pose_flags(),
        "seed={seed} anchor={anchor}"
    );
    assert_eq!(
        physics.state().unwrap().swim_pose_active,
        expected.state().unwrap().swim_pose_active,
        "seed={seed} anchor={anchor}"
    );
}

#[test]
fn seeded_climber_streams_match_live_execution_after_arbitrary_replay() {
    // Run the existing input streams both without shapes and after a scaffold teleport.
    for case in 0..64 {
        let seed = case % 32 + 1;
        let mut random = Random(seed);
        let world = ContactWorld {
            seed: random.next(),
            collision: case >= 32,
        };
        let mut physics = due_physics();
        let first = sim::MovementInput::default();
        let teleported = physics.advance(simulation_clock::TICK_DURATION, first, &world);
        if world.collision {
            assert_eq!(
                [
                    teleported.samples[0].position[0],
                    teleported.samples[0].position[2]
                ],
                [0.5, 0.5],
                "seed={seed}: teleport must stay inside the scaffold column",
            );
        }
        let mut ticks = vec![ModelTick {
            input: first,
            entry: 0,
            edits: Vec::new(),
            server_flags: Default::default(),
        }];
        let mut pending = Vec::<Edit>::new();
        for _ in 0..20 {
            let current = physics.state().unwrap().tick;
            for _ in 0..random.next() % 4 {
                let stamp = match random.next() % 4 {
                    0 => 0,
                    1 => current,
                    2 => current + 1 + random.next() % 100,
                    _ => 101 + random.next() % (current - 100),
                };
                let word = random.next() & ClimberFlags::mask();
                let historical = stamp != 0 && stamp < current;
                let baseline = if historical {
                    ticks[(stamp - 100) as usize].entry
                } else {
                    flags(&physics)
                };
                let edit = Edit {
                    mask: (baseline ^ word) & ClimberFlags::mask(),
                    word,
                };
                if historical {
                    ticks[(stamp - 100) as usize].edits.push(edit);
                    for pending in &mut pending {
                        pending.mask &= !edit.mask;
                    }
                } else {
                    pending.push(edit);
                }
                let rewind = physics.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        extended_flags: Some(word),
                        ..Default::default()
                    },
                );
                assert_eq!(rewind, (historical && edit.mask != 0).then_some(stamp));
                if let Some(anchor) = rewind {
                    check_replay(&mut physics, anchor, &ticks, &pending, &world, seed);
                }
            }
            // A mobile tail guarantees missing terrain rejects this rewind.
            if current > 101 && !ticks.last().unwrap().input.immobile && random.next() & 3 == 0 {
                let stamp = 101 + random.next() % (current - 101);
                let word = random.next() & ClimberFlags::mask();
                let mask = (ticks[(stamp - 100) as usize].entry ^ word) & ClimberFlags::mask();
                let rewind = physics.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        extended_flags: Some(word),
                        ..Default::default()
                    },
                );
                assert_eq!(rewind, (mask != 0).then_some(stamp));
                if let Some(anchor) = rewind {
                    assert!(
                        physics
                            .replay_retained_from(anchor, &super::climber_replay::MissingTerrain)
                            .is_err()
                    );
                }
                // A rejected rewind leaves this write for the next live input instead.
                pending.push(Edit { mask, word });
                if mask != 0 && current > 102 && random.next() & 1 == 0 {
                    let newer_stamp = if stamp == 101 { current - 1 } else { stamp - 1 };
                    let word = ticks[(newer_stamp - 100) as usize].entry ^ mask;
                    // A newer accepted write owns the old pending bits before either input runs.
                    for pending in &mut pending {
                        pending.mask &= !mask;
                    }
                    ticks[(newer_stamp - 100) as usize]
                        .edits
                        .push(Edit { mask, word });
                    assert_eq!(
                        physics.apply_server_movement_flags(
                            newer_stamp,
                            client_world::MovementFlagUpdate {
                                extended_flags: Some(word),
                                ..Default::default()
                            }
                        ),
                        Some(newer_stamp)
                    );
                    check_replay(
                        &mut physics,
                        stamp.min(newer_stamp),
                        &ticks,
                        &pending,
                        &world,
                        seed,
                    );
                }
            }
            let choices = random.next();
            let poses = physics.pose_flags();
            let spinning = physics.state().unwrap().spin_attack_ticks.is_some();
            let server_flags = match choices % 8 {
                0 => client_world::MovementFlagUpdate::default(),
                1 => client_world::MovementFlagUpdate {
                    swimming: Some(!poses.swimming),
                    ..Default::default()
                },
                2 => client_world::MovementFlagUpdate {
                    crawling: Some(!poses.crawling),
                    ..Default::default()
                },
                3 => client_world::MovementFlagUpdate {
                    gliding: Some(!poses.gliding),
                    ..Default::default()
                },
                4 => client_world::MovementFlagUpdate {
                    spinning: Some(!spinning),
                    ..Default::default()
                },
                5 => client_world::MovementFlagUpdate {
                    swimming: Some(poses.swimming),
                    crawling: Some(poses.crawling),
                    gliding: Some(poses.gliding),
                    spinning: Some(spinning),
                    ..Default::default()
                },
                6 => client_world::MovementFlagUpdate {
                    swimming: Some(choices & 8 != 0),
                    ..Default::default()
                },
                _ => client_world::MovementFlagUpdate {
                    crawling: Some(choices & 8 != 0),
                    ..Default::default()
                },
            };
            assert_eq!(
                physics.apply_server_movement_flags(current, server_flags),
                None
            );
            let input = sim::MovementInput {
                immobile: choices & 16 != 0,
                jumping: choices & 32 != 0,
                sneaking: random.next() & 1 != 0,
                forward: (random.next() % 3) as f64 - 1.0,
                strafe: (random.next() % 3) as f64 - 1.0,
                ..Default::default()
            };
            let entry = flags(&physics);
            let frame = physics.advance(simulation_clock::TICK_DURATION, input, &world);
            assert_eq!(frame.completed_ticks, 1);
            ticks.push(ModelTick {
                input,
                entry,
                edits: std::mem::take(&mut pending),
                // Matching server bits are no-ops, even if an earlier climber edit later
                // changes the pose this reference run reaches before the write.
                server_flags: client_world::MovementFlagUpdate {
                    swimming: server_flags.swimming.filter(|flag| *flag != poses.swimming),
                    crawling: server_flags.crawling.filter(|flag| *flag != poses.crawling),
                    gliding: server_flags.gliding.filter(|flag| *flag != poses.gliding),
                    spinning: server_flags.spinning.filter(|flag| *flag != spinning),
                    ..Default::default()
                },
            });
            let (expected, samples) = run_live_model(&ticks, &pending, &world);
            assert_eq!(
                frame.samples[0].movement,
                samples.last().unwrap().movement,
                "seed={seed} tick={}",
                current + 1
            );
            assert_eq!(
                flags(&physics),
                flags(&expected),
                "seed={seed} tick={}",
                current + 1
            );
            let anchor = 101 + random.next() % ticks.len() as u64;
            check_replay(&mut physics, anchor, &ticks, &pending, &world, seed);
            for start in 101..=current + 1 {
                check_replay(&mut physics.clone(), start, &ticks, &pending, &world, seed);
            }
        }

        // Every live set capture must still admit a clear after arbitrary replay history.
        for index in 1..ticks.len() {
            let bit = 1 << (ActorFlag::OverDescendingBlock.wire_value() % 64);
            if ticks[index].entry & bit == 0 {
                continue;
            }
            let mut cleared = physics.clone();
            let stamp = 100 + index as u64;
            let word = ticks[index].entry & !bit;
            assert_eq!(
                cleared.apply_server_movement_flags(
                    stamp,
                    client_world::MovementFlagUpdate {
                        extended_flags: Some(word),
                        ..Default::default()
                    }
                ),
                Some(stamp)
            );
            let mut corrected_ticks = ticks.clone();
            corrected_ticks[index]
                .edits
                .push(Edit { mask: bit, word: 0 });
            for anchor in [stamp, 101, stamp] {
                check_replay(&mut cleared, anchor, &corrected_ticks, &[], &world, seed);
            }
        }
    }
}
