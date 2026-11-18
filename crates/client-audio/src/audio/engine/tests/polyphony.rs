use super::*;

/// A synthetic chord uses one instrument sample at several independent pitches.
fn chord(name: &str) -> impl Iterator<Item = SoundRequest> + '_ {
    (0..14).map(move |semitone| {
        SoundRequest::new(name).scaled(0.8, 2.0_f32.powf((semitone as f32 - 7.0) / 12.0))
    })
}

/// Cached starts retain each requested pitch and its independent playback length.
#[test]
fn polyphonic_notes_keep_each_pitch_when_the_sample_is_warm() {
    let name = "note.flute";
    let mut engine = engine(&[(name, "record")]);
    let settings = AudioSettings::default();
    for request in chord(name) {
        engine.enqueue(request);
    }
    let mut sources = engine.pump(None, 0.0, &settings);
    assert_eq!(sources.len(), 14);
    assert_eq!(engine.stats.voice_limit, 0);
    // Keep and consume the sources independently; a new note must not cancel an older one.
    let expected_lengths: Vec<_> = chord(name)
        .map(|request| {
            let pitch = request.pitch.min;
            (4800.0_f64 / f64::from(pitch)).ceil() as usize * 2
        })
        .collect();
    for (source, expected) in sources.iter_mut().zip(expected_lengths) {
        let length = source.count();
        assert!(length.abs_diff(expected) <= 2, "{length} versus {expected}");
    }
}

/// Sharing one decode preserves every deferred start and its requested pitch.
#[test]
fn polyphonic_notes_survive_a_shared_cold_decode_without_losing_starts() {
    let name = "note.polyphonic_cold";
    let (mut engine, path) = engine_with_encoded(name, "record");
    let settings = AudioSettings::default();
    for request in chord(name) {
        engine.enqueue(request);
    }
    let mut sources = engine.pump(None, 0.0, &settings);
    assert_eq!(sources.len() + engine.pending.len(), 14);
    test_time::eventually(
        "all independently pitched notes to finish the shared decode",
        || {
            sources.extend(engine.pump(None, 0.0, &settings));
            engine.pending.is_empty()
        },
    );
    assert_eq!(sources.len(), 14);
    for (source, request) in sources.iter().zip(chord(name)) {
        assert_eq!(source.pitch(), request.pitch.min);
    }
    assert_eq!(engine.stats.voice_limit, 0);
    assert_eq!(engine.stats.queue_overflow, 0);
    let _ = std::fs::remove_file(path);
}

/// Removing the per-sound cap does not bypass the engine's overall voice budget.
#[test]
fn polyphonic_notes_still_obey_the_overall_voice_budget() {
    let name = "note.flute";
    let mut engine = engine(&[(name, "record")]);
    let excess = 7;
    for _ in 0..MAX_VOICES + excess {
        engine.enqueue(SoundRequest::new(name));
    }
    let sources = engine.pump(None, 0.0, &AudioSettings::default());
    assert_eq!(sources.len(), MAX_VOICES);
    assert_eq!(engine.voices.len(), MAX_VOICES);
    assert_eq!(engine.stats.voice_limit, excess as u64);
}
