//! Read-only managed-server observations, gated independently from current-world terrain.
use super::{State, cinnabar};
use cinnabar::extension::local_terrain::{self as api, ReadError, ReadResult};
use mod_api::local_terrain as native;
use std::sync::Arc;

impl crate::ModHost {
    /// Installs only the source belonging to the current managed local connection.
    pub fn set_local_terrain_source(&mut self, source: Option<Arc<dyn native::Source>>) {
        let state = self.instance.store.data_mut();
        state.local_terrain = source.filter(|_| state.grants.local_terrain);
    }
}

impl api::Host for State {
    fn read_region(
        &mut self,
        query: api::Query,
        after: Option<api::Stamp>,
    ) -> anyhow::Result<Result<ReadResult, ReadError>> {
        self.terrain.calls += 1;
        if self.terrain.calls > super::MAX_IMPORT_WRITES as usize {
            return Ok(Err(ReadError::BudgetExhausted));
        }
        if !self.grants.local_terrain || query.load && !self.grants.local_terrain_load {
            return Ok(Err(ReadError::Denied));
        }
        if !self.world_output_available()
            || !self
                .player_state
                .world
                .as_ref()
                .is_some_and(|world| world.alive)
        {
            return Ok(Err(ReadError::Unavailable));
        }
        let query = native::Query {
            dimension: query.dimension,
            min: [query.min.x, query.min.y, query.min.z],
            size: [query.size.0, query.size.1, query.size.2],
            load: query.load,
        };
        if !query.valid() {
            return Ok(Err(ReadError::InvalidQuery));
        }
        let Some(source) = &self.local_terrain else {
            return Ok(Err(ReadError::Unavailable));
        };
        Ok(match source.read(&query) {
            Ok(Some(region)) if region.valid(&query) => {
                let stamp = api::Stamp {
                    session: region.session,
                    revision: region.revision,
                };
                if after.as_ref() == Some(&stamp) {
                    Ok(ReadResult::Unchanged(stamp))
                } else {
                    Ok(ReadResult::Changed(api::Region {
                        stamp,
                        palette: region
                            .palette
                            .iter()
                            .map(|state| api::BlockState {
                                name: state.name.clone(),
                                states: state.states.clone(),
                            })
                            .collect(),
                        cells: region.cells.clone(),
                    }))
                }
            }
            Ok(Some(_)) => Err(ReadError::Unavailable),
            Ok(None) => Ok(ReadResult::Pending),
            Err(native::Error::Unavailable) => Err(ReadError::Unavailable),
            Err(native::Error::InvalidQuery) => Err(ReadError::InvalidQuery),
            Err(native::Error::BudgetExhausted) => Err(ReadError::BudgetExhausted),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::Host as _;

    #[test]
    fn local_server_reads_require_independent_observation_and_loading_consent() {
        let mut state = State::new(Default::default(), String::new(), Default::default());
        let mut query = api::Query {
            dimension: 1,
            min: super::super::cinnabar::extension::terrain::Position { x: 0, y: 64, z: 0 },
            size: (1, 1, 1),
            load: false,
        };
        assert_eq!(
            state.read_region(query.clone(), None).unwrap(),
            Err(ReadError::Denied)
        );
        state.grants.local_terrain = true;
        query.load = true;
        assert_eq!(
            state.read_region(query.clone(), None).unwrap(),
            Err(ReadError::Denied)
        );
        state.grants.local_terrain_load = true;
        assert_eq!(
            state.read_region(query, None).unwrap(),
            Err(ReadError::Unavailable)
        );
    }
}
