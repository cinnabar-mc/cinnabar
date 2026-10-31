//! Bounded sound observations between playback and authorized personal components.
use client_audio::audio::engine::SoundEvent;
use client_extensions::modding::multi::ModHosts;

/// Offers bounded observations in order; excess or oversized requests play normally.
pub(super) fn apply(hosts: &mut ModHosts, observations: &[SoundEvent]) -> Vec<bool> {
    let selected: Vec<_> = observations
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.name.len() <= mod_api::MAX_SOUND_NAME_BYTES
                && event
                    .position
                    .is_none_or(|p| p.iter().all(|v| v.is_finite()))
        })
        .take(mod_api::MAX_SOUND_FILTER_EVENTS)
        .collect();
    let events = selected
        .iter()
        .map(|(_, event)| mod_host::SoundEvent {
            name: event.name.to_string(),
            position: event.position.map(|p| mod_host::SoundPosition {
                x: p[0],
                y: p[1],
                z: p[2],
            }),
            looping: event.looping,
        })
        .collect::<Vec<_>>();
    let mut accepted = vec![true; observations.len()];
    if !events.is_empty() {
        for ((index, _), play) in selected.into_iter().zip(hosts.filter_sounds(&events)) {
            accepted[index] = play;
        }
    }
    accepted
}
