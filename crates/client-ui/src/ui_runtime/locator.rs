//! Locator state comes only from server updates, including explicit hiding.

use std::collections::BTreeMap;

use protocol::{LocatorEvent, LocatorWaypointUpdate};
use ui_screens::hud_renderers::{LocatorMarker, LocatorPaint, LocatorView};

/// Experience changes temporarily take the locator's place.
pub(super) const EXPERIENCE_HOLD_MILLIS: u64 = 5_000;

/// Locations are retired together with the owning UI session.
#[derive(Clone, Debug, Default)]
pub(super) struct LocatorState {
    players: BTreeMap<i64, ([f32; 3], i32)>,
    waypoints: BTreeMap<uuid::Uuid, LocatorWaypointUpdate>,
    actor_positions: BTreeMap<i64, ([f32; 3], i32)>,
    experience_until: u64,
    pub skipped: u64,
}

impl LocatorState {
    /// Applies partial waypoint updates and removals without synthesizing hidden players.
    pub fn apply(&mut self, event: LocatorEvent) {
        match event {
            LocatorEvent::Player {
                actor_unique_id,
                position: None,
                ..
            } => {
                self.players.remove(&actor_unique_id);
            }
            LocatorEvent::Player {
                actor_unique_id,
                position: Some(position),
                dimension,
            } => {
                if position.iter().all(|value| value.is_finite()) {
                    self.players.insert(actor_unique_id, (position, dimension));
                } else {
                    self.skipped = self.skipped.saturating_add(1);
                }
            }
            LocatorEvent::Waypoint {
                id, update: None, ..
            } => {
                self.waypoints.remove(&id);
            }
            LocatorEvent::Waypoint {
                id,
                replace,
                update: Some(update),
            } => {
                self.skipped = self
                    .skipped
                    .saturating_add(u64::from(update.skipped_fields));
                let target = self.waypoints.entry(id).or_default();
                if replace {
                    *target = LocatorWaypointUpdate::default();
                }
                macro_rules! merge { ($($field:ident),+) => {$(
                    if update.$field.is_some() { target.$field = update.$field; }
                )+}; }
                merge!(
                    visible,
                    texture,
                    color,
                    actor_unique_id,
                    client_position_authority
                );
                if let Some((position, dimension)) = update.position {
                    if position.iter().all(|value| value.is_finite()) {
                        target.position = Some((position, dimension));
                    } else {
                        self.skipped = self.skipped.saturating_add(1);
                    }
                }
                if let Some(size) = update.size {
                    if size.iter().all(|value| value.is_finite() && *value > 0.0) {
                        target.size = Some(size);
                    } else {
                        self.skipped = self.skipped.saturating_add(1);
                    }
                }
            }
        }
    }

    /// Starts the temporary experience display on a changed authoritative XP value.
    pub fn experience_changed(&mut self, now: u64) {
        self.experience_until = now.saturating_add(EXPERIENCE_HOLD_MILLIS);
    }

    /// Samples only actors explicitly authorized by the server's waypoint updates.
    pub fn observe_actors(
        &mut self,
        dimension: i32,
        mut resolve: impl FnMut(i64) -> Option<[f32; 3]>,
    ) {
        self.actor_positions.clear();
        for point in self.waypoints.values().filter(|point| {
            point.visible != Some(false) && point.client_position_authority == Some(true)
        }) {
            if let Some(id) = point.actor_unique_id
                && let Some(position) = resolve(id)
                && position.iter().all(|value| value.is_finite())
            {
                self.actor_positions.insert(id, (position, dimension));
            }
        }
    }

    /// Resolves client-position authority before falling back to a supplied server position.
    fn waypoint_position(&self, point: &LocatorWaypointUpdate) -> Option<([f32; 3], i32)> {
        if point.client_position_authority == Some(true)
            && let Some(position) = point
                .actor_unique_id
                .and_then(|id| self.actor_positions.get(&id))
        {
            return Some(*position);
        }
        point.position
    }

    /// Whether any received location belongs on the current dimension's locator.
    pub fn visible(&self, dimension: i32, now: u64) -> bool {
        now >= self.experience_until
            && (self.players.values().any(|(_, dim)| *dim == dimension)
                || self.waypoints.values().any(|point| {
                    point.visible != Some(false)
                        && self
                            .waypoint_position(point)
                            .is_some_and(|(_, dim)| dim == dimension)
                }))
    }

    /// Captures only explicitly supplied locations; ordinary actor observations do not reveal players.
    pub fn paint(&self, view: LocatorView, dimension: i32) -> LocatorPaint {
        let mut markers: Vec<_> = self
            .players
            .iter()
            .filter(|(_, (_, dim))| *dim == dimension)
            .map(|(id, (position, _))| LocatorMarker {
                position: *position,
                // Stable presentation color while legacy location packets carry no color.
                color: legacy_color(*id),
                texture: None,
                size: None,
            })
            .collect();
        markers.extend(self.waypoints.values().filter_map(|point| {
            let (position, dim) = self.waypoint_position(point)?;
            (point.visible != Some(false) && dim == dimension).then(|| LocatorMarker {
                position,
                color: point.color.unwrap_or([255; 4]),
                texture: point.texture.clone(),
                size: point.size,
            })
        }));
        LocatorPaint { view, markers }
    }
}

impl super::UiRuntime {
    /// Updates explicitly tracked waypoint actors from the current presentation sample.
    pub fn observe_locator_actors(
        &mut self,
        dimension: i32,
        resolve: impl FnMut(i64) -> Option<[f32; 3]>,
    ) {
        self.session.locator.observe_actors(dimension, resolve);
    }
}

/// Produces a bright stable color from the legacy actor identity.
fn legacy_color(id: i64) -> [u8; 4] {
    let hash = (id as u64).wrapping_mul(0x9e3779b97f4a7c15);
    [
        96 + ((hash >> 16) as u8 >> 1),
        96 + ((hash >> 32) as u8 >> 1),
        96 + ((hash >> 48) as u8 >> 1),
        255,
    ]
}

#[cfg(test)]
mod tests;
