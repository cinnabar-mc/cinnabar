//! Bounded sound playback requests offered to authorized sound handlers.
use client_audio::audio::engine::SoundEvent;
use client_extensions::modding::multi::ModHosts;

/// Offers bounded requests in order; excess or oversized requests play unhandled.
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
        .take(mod_api::MAX_HANDLED_SOUNDS)
        .collect();
    let sounds = selected
        .iter()
        .map(|(_, event)| mod_host::SoundContext {
            name: event.name.to_string(),
            position: event.position.map(|p| mod_host::SoundPosition {
                x: p[0],
                y: p[1],
                z: p[2],
            }),
            looping: event.looping,
            cancelled: false,
        })
        .collect::<Vec<_>>();
    let mut accepted = vec![true; observations.len()];
    if !sounds.is_empty() {
        for ((index, _), play) in selected.into_iter().zip(hosts.handle_sounds(sounds)) {
            accepted[index] = play;
        }
    }
    accepted
}
