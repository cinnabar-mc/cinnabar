use super::{State, cinnabar};
use crate::terrain::Sample;
use crate::{
    TerrainPosition, TerrainQuery, TerrainReadError, TerrainReadResult, TerrainRegion, TerrainStamp,
};
use anyhow::{Result, bail};

impl cinnabar::extension::terrain::Host for State {
    fn read_region(
        &mut self,
        query: TerrainQuery,
        after: Option<TerrainStamp>,
    ) -> Result<Result<TerrainReadResult, TerrainReadError>> {
        self.terrain.calls += 1;
        if self.terrain.calls > super::MAX_IMPORT_WRITES as usize {
            bail!("terrain read budget exhausted");
        }
        if !self.grants.terrain {
            return Ok(Err(TerrainReadError::Denied));
        }
        // Admission and sampling both measure from the observed feet of the terrain session.
        let (Some(_), Some(world)) = (&self.snapshot, &self.player_state.world) else {
            return Ok(Err(TerrainReadError::Unavailable));
        };
        let (x, y, z) = world.position;
        let scope = (world.session, world.dimension);
        if let Err(error) = crate::terrain_query_size(&query, [x, y, z]) {
            return Ok(Err(error));
        }
        if !self.terrain.pending_queries.contains(&query) {
            if self.terrain.subscriptions_full() {
                return Ok(Err(TerrainReadError::BudgetExhausted));
            }
            self.terrain.pending_queries.push(query);
        }
        let sample = self
            .terrain
            .subscriptions
            .iter()
            .find(|(candidate, _)| *candidate == query)
            .map(|(_, sample)| sample);
        Ok(match sample {
            Some(Sample::Failed(error)) => Err(*error),
            Some(Sample::Region(region))
                if (region.stamp.session, region.stamp.dimension) == scope =>
            {
                Ok(if after == Some(region.stamp) {
                    TerrainReadResult::Unchanged(region.stamp)
                } else {
                    TerrainReadResult::Changed(TerrainRegion::clone(region))
                })
            }
            _ => Ok(TerrainReadResult::Pending),
        })
    }

    /// Subscribes a search and returns its latest answer in the current world: matching loaded
    /// blocks found so far, nearest the feet first. A new search returns none.
    fn find_blocks(
        &mut self,
        identifiers: Vec<String>,
        range: f32,
        limit: u32,
    ) -> Result<Result<Vec<TerrainPosition>, TerrainReadError>> {
        self.terrain.calls += 1;
        if self.terrain.calls > super::MAX_IMPORT_WRITES as usize {
            bail!("terrain read budget exhausted");
        }
        if !self.grants.terrain {
            return Ok(Err(TerrainReadError::Denied));
        }
        let (Some(_), Some(world)) = (&self.snapshot, &self.player_state.world) else {
            return Ok(Err(TerrainReadError::Unavailable));
        };
        let scope = (world.session, world.dimension);
        let search = match crate::BlockSearch::admit(identifiers, range, limit) {
            Ok(search) => search,
            Err(error) => return Ok(Err(error)),
        };
        if !self.terrain.pending_searches.contains(&search) {
            if self.terrain.subscriptions_full() {
                return Ok(Err(TerrainReadError::BudgetExhausted));
            }
            self.terrain.pending_searches.push(search.clone());
        }
        let sample = self
            .terrain
            .searches
            .iter()
            .find(|(candidate, _)| *candidate == search)
            .and_then(|(_, sample)| sample.as_ref());
        Ok(match sample {
            Some(Err(error)) => Err(*error),
            Some(Ok(found)) if (found.session, found.dimension) == scope => Ok(found
                .positions
                .iter()
                .take(search.limit as usize)
                .copied()
                .collect()),
            _ => Ok(Vec::new()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        GameplaySnapshot, GameplayVector3, ModGrants, TerrainGeometry, TerrainPosition,
        TerrainRefresh, TerrainRegion,
    };
    use cinnabar::extension::terrain::Host as _;
    fn state(grants: ModGrants) -> State {
        let mut value = State::new(grants, String::new(), Default::default());
        value.snapshot = Some(GameplaySnapshot {
            session: 1,
            dimension: 0,
            eye: GameplayVector3 {
                x: 0.,
                y: 1.62,
                z: 0.,
            },
            yaw: 0.,
            pitch: 0.,
            attack_held: false,
            frame_seconds: 0.016,
            players: vec![],
        });
        value.player_state.world = Some(crate::WorldObservation {
            session: 1,
            dimension: 0,
            world: "fixture".into(),
            position: (0., 0., 0.),
            alive: true,
            runtime_id: 1,
        });
        value
    }
    fn query(x: i32) -> TerrainQuery {
        TerrainQuery {
            min: TerrainPosition { x, y: 0, z: 0 },
            max: TerrainPosition {
                x: x + 1,
                y: 1,
                z: 1,
            },
            geometry: TerrainGeometry::COLLISION,
        }
    }
    fn region(session: u64, dimension: i32) -> TerrainRegion {
        TerrainRegion {
            stamp: TerrainStamp {
                session,
                dimension,
                registry: 0,
                revision: 0,
            },
            chunks: vec![],
            layer_count: 1,
            loaded: vec![1],
            states: vec![0],
            palette: vec![],
            shape_offsets: vec![0, 0],
            shape_boxes: vec![],
            overrides: vec![],
        }
    }
    /// Commits the staged query, then supplies `refresh` for it as the next frame would.
    fn sample(value: &mut State, refresh: TerrainRefresh) {
        value.terrain.commit();
        value.terrain.refresh(0, |_, _| refresh);
        value.terrain.begin_frame();
    }
    #[test]
    fn terrain_is_denied_by_default() {
        let mut value = state(ModGrants::default());
        assert_eq!(
            value.read_region(query(0), None).unwrap(),
            Err(TerrainReadError::Denied)
        );
        assert!(value.terrain.pending_queries.is_empty());
    }
    #[test]
    fn reads_deduplicate_and_deliver_changed_then_unchanged() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        for _ in 0..2 {
            assert_eq!(
                value.read_region(query(0), None).unwrap(),
                Ok(TerrainReadResult::Pending)
            );
        }
        assert_eq!(value.terrain.pending_queries.len(), 1);
        sample(&mut value, TerrainRefresh::Replace(Box::new(region(1, 0))));
        let Ok(TerrainReadResult::Changed(first)) = value.read_region(query(0), None).unwrap()
        else {
            panic!("a sample was supplied");
        };
        assert_eq!(
            value.read_region(query(0), Some(first.stamp)).unwrap(),
            Ok(TerrainReadResult::Unchanged(first.stamp))
        );
        sample(&mut value, TerrainRefresh::Replace(Box::new(region(1, 0))));
        assert!(matches!(
            value.read_region(query(0), Some(first.stamp)).unwrap(),
            Ok(TerrainReadResult::Changed(next)) if next.stamp.revision > first.stamp.revision
        ));
        sample(
            &mut value,
            TerrainRefresh::Failed(TerrainReadError::LimitExceeded),
        );
        assert_eq!(
            value.read_region(query(0), None).unwrap(),
            Err(TerrainReadError::LimitExceeded)
        );
    }
    #[test]
    fn invalid_and_excess_queries_are_typed_errors() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        let mut empty = query(0);
        empty.max.x = 0;
        assert_eq!(
            value.read_region(empty, None).unwrap(),
            Err(TerrainReadError::InvalidQuery)
        );
        assert_eq!(
            value.read_region(query(500), None).unwrap(),
            Err(TerrainReadError::OutOfRange)
        );
        for x in 0..mod_api::MAX_TERRAIN_QUERIES as i32 {
            assert!(value.read_region(query(x), None).unwrap().is_ok());
        }
        assert_eq!(
            value.read_region(query(-1), None).unwrap(),
            Err(TerrainReadError::BudgetExhausted)
        );
        value.player_state.world = None;
        assert_eq!(
            value.read_region(query(0), None).unwrap(),
            Err(TerrainReadError::Unavailable)
        );
    }
    /// A query admitted against the feet is one the client can sample, and the eye's height
    /// above the feet never admits a query the client would reject.
    #[test]
    fn admission_and_sampling_share_the_feet_origin() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        let feet = [0., 0., 0.];
        for (y, admitted) in [(-128, true), (128, true), (129, false)] {
            let mut query = query(0);
            query.min.y = y;
            query.max.y = y + 1;
            let read = value.read_region(query, None).unwrap();
            assert_eq!(read.is_ok(), admitted, "{y}");
            assert_eq!(crate::terrain_query_size(&query, feet).is_ok(), admitted);
            value.terrain.begin_frame();
        }
    }
    #[test]
    fn terrain_uses_the_observed_world_session_instead_of_the_actor_session() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        value.player_state.world = Some(crate::WorldObservation {
            session: 7,
            dimension: 2,
            world: "fixture".into(),
            position: (0., 0., 0.),
            alive: true,
            runtime_id: 1,
        });
        value.read_region(query(0), None).unwrap().unwrap();
        for (session, dimension, delivered) in [(7, 2, true), (1, 2, false), (7, 0, false)] {
            sample(
                &mut value,
                TerrainRefresh::Replace(Box::new(region(session, dimension))),
            );
            let result = value.read_region(query(0), None).unwrap().unwrap();
            assert_eq!(
                matches!(result, TerrainReadResult::Changed(_)),
                delivered,
                "{session} {dimension}"
            );
        }
    }

    fn ores() -> Vec<String> {
        vec![
            "minecraft:diamond_ore".into(),
            "minecraft:ancient_debris".into(),
        ]
    }
    fn position(x: i32) -> TerrainPosition {
        TerrainPosition { x, y: 0, z: 0 }
    }
    /// Answers the committed search as the client would before the next frame.
    fn answer(value: &mut State, answer: Result<crate::FoundBlocks, TerrainReadError>) {
        value.terrain.commit();
        for (_, sample) in &mut value.terrain.searches {
            *sample = Some(answer.clone());
        }
        value.terrain.begin_frame();
    }
    #[test]
    fn find_blocks_needs_terrain_authority_and_a_gameplay_frame() {
        let mut denied = state(ModGrants::default());
        assert_eq!(
            denied.find_blocks(ores(), 32.0, 8).unwrap(),
            Err(TerrainReadError::Denied)
        );
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        value.snapshot = None;
        assert_eq!(
            value.find_blocks(ores(), 32.0, 8).unwrap(),
            Err(TerrainReadError::Unavailable)
        );
        assert!(value.terrain.pending_searches.is_empty());
    }
    #[test]
    fn find_blocks_subscribes_then_returns_the_nearest_matches_in_the_current_world() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        assert_eq!(value.find_blocks(ores(), 32.0, 2).unwrap(), Ok(Vec::new()));
        let reordered = vec![ores()[1].clone(), ores()[0].clone()];
        assert_eq!(
            value.find_blocks(reordered, 32.0, 2).unwrap(),
            Ok(Vec::new())
        );
        assert_eq!(
            value.terrain.pending_searches.len(),
            1,
            "equal searches share one"
        );
        let nearest = vec![position(1), position(-2), position(3)];
        answer(
            &mut value,
            Ok(crate::FoundBlocks {
                session: 1,
                dimension: 0,
                positions: nearest.clone(),
            }),
        );
        assert_eq!(
            value.find_blocks(ores(), 32.0, 2).unwrap(),
            Ok(nearest[..2].to_vec())
        );
        answer(
            &mut value,
            Ok(crate::FoundBlocks {
                session: 1,
                dimension: -1,
                positions: nearest,
            }),
        );
        assert_eq!(
            value.find_blocks(ores(), 32.0, 2).unwrap(),
            Ok(Vec::new()),
            "an answer from another world is never delivered"
        );
        answer(&mut value, Err(TerrainReadError::LimitExceeded));
        assert_eq!(
            value.find_blocks(ores(), 32.0, 2).unwrap(),
            Err(TerrainReadError::LimitExceeded)
        );
        value.terrain.commit();
        value.terrain.begin_frame();
        value.terrain.commit();
        assert!(
            value.terrain.searches.is_empty(),
            "a search not renewed is unsubscribed"
        );
    }
    #[test]
    fn find_blocks_shares_the_terrain_query_and_call_budgets() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        assert_eq!(
            value
                .find_blocks(ores(), mod_api::MAX_TERRAIN_DISTANCE as f32 + 1.0, 1)
                .unwrap(),
            Err(TerrainReadError::OutOfRange)
        );
        assert_eq!(
            value
                .find_blocks(ores(), 8.0, mod_api::MAX_FOUND_BLOCKS as u32 + 1)
                .unwrap(),
            Err(TerrainReadError::LimitExceeded)
        );
        for x in 0..mod_api::MAX_TERRAIN_QUERIES as i32 - 1 {
            assert!(value.read_region(query(x), None).unwrap().is_ok());
        }
        assert!(value.find_blocks(ores(), 16.0, 1).unwrap().is_ok());
        assert_eq!(
            value.find_blocks(ores(), 24.0, 1).unwrap(),
            Err(TerrainReadError::BudgetExhausted)
        );
        assert_eq!(
            value.read_region(query(-1), None).unwrap(),
            Err(TerrainReadError::BudgetExhausted)
        );
        assert!(
            value.find_blocks(ores(), 16.0, 1).is_err(),
            "eight terrain calls per callback"
        );
    }
}
