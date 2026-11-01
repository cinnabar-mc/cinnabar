use super::*;

/// Uses optional installed carriers in place; no game audio belongs in this fixture.
#[test]
fn installed_toast_sound_reaches_the_mixer() {
    let Some(catalog_path) = std::env::var_os("CINNABAR_TEST_AUDIO_CATALOG") else {
        eprintln!(
            "skipping installed_toast_sound_reaches_the_mixer: missing CINNABAR_TEST_AUDIO_CATALOG"
        );
        return;
    };
    let Some(bank_path) = std::env::var_os("CINNABAR_TEST_SOUND_BANK") else {
        eprintln!(
            "skipping installed_toast_sound_reaches_the_mixer: missing CINNABAR_TEST_SOUND_BANK"
        );
        return;
    };
    for path in [&catalog_path, &bank_path] {
        if !std::path::Path::new(path).is_file() {
            eprintln!(
                "skipping installed_toast_sound_reaches_the_mixer: missing fixture {}",
                std::path::Path::new(path).display()
            );
            return;
        }
    }
    let catalog = RuntimeAudioCatalog::decode(&std::fs::read(catalog_path).unwrap()).unwrap();
    let bank = SoundBank::open(std::path::Path::new(&bank_path), Some(Arc::new(catalog)))
        .unwrap()
        .expect("installed sound bank");
    let mut engine = AudioEngine::new(Some(bank));
    engine.enqueue(SoundRequest::new(client_ui::sound_requests::TOAST));
    let settings = AudioSettings::default();
    let mut sources = Vec::new();
    test_time::eventually("toast sample decoded and started", || {
        sources = engine.pump(None, 0.0, &settings);
        !sources.is_empty() || engine.stats.no_pcm != 0 || engine.stats.no_definition != 0
    });
    assert_eq!(sources.len(), 1, "{:?}", engine.stats);
    let (mut device, mixer) = crate::named_audio::AudioDevice::memory_mixer();
    assert!(device.play_source(sources.pop().unwrap()));
    assert!(
        mixer
            .take(crate::audio::OUTPUT_RATE as usize)
            .any(|sample| sample.abs() > 0.001)
    );
    assert_eq!(engine.stats.started, 1);
}
