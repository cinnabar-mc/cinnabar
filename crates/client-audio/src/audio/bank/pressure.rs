//! Decoded sounds released under host memory pressure; later lookups decode them again.

use memory_pressure::MemoryPressure;

use super::{CACHE_BUDGET_BYTES, SoundBank};

/// Decoded bytes kept through low pressure, so the sounds in steadiest use do not decode again.
const LOW_PRESSURE_CACHE_BYTES: usize = CACHE_BUDGET_BYTES / 4;

impl SoundBank {
    /// Releases decoded sounds for `pressure` and returns the cached bytes dropped.
    ///
    /// Low pressure evicts the oldest sounds down to a quarter of the cache budget, and critical
    /// pressure empties the cache; both leave running decodes for the sounds waiting on them.
    /// Background pressure also drops unclaimed streams and abandons queued decodes. Playing
    /// voices keep their own samples, and the next lookup of a released sound queues its decode.
    pub fn release_decoded(&mut self, pressure: MemoryPressure) -> usize {
        let cached = self.cache_bytes;
        self.evict_to(if pressure.releases_everything() {
            0
        } else {
            LOW_PRESSURE_CACHE_BYTES
        });
        if pressure == MemoryPressure::Background {
            self.ready_streams.clear();
            self.abandon_decodes();
        }
        cached - self.cache_bytes
    }

    /// Decoded bytes the cache currently holds.
    #[cfg(test)]
    pub(crate) fn cached_bytes(&self) -> usize {
        self.cache_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        DECODE_WORKERS, Decoder, MAX_QUEUED_DECODES, Pcm, SoundBankIndex, SoundEventTables,
        sync_channel,
    };
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    };

    /// A bank with no files, for exercising the decoded cache alone.
    fn bank() -> SoundBank {
        let encoded = assets::sound_bank::encode_sound_bank(b"{}", b"{}", b"{}", &[]).unwrap();
        SoundBank::from_parts(
            SoundBankIndex::decode_prefix(&encoded).unwrap(),
            SoundEventTables::default(),
            None,
        )
    }

    /// Decoded mono samples occupying `bytes` of the cache.
    fn decoded(bytes: usize) -> Pcm {
        Pcm {
            channels: 1,
            rate: 48_000,
            samples: vec![0; bytes / 2].into(),
        }
    }

    /// [`decoded`] samples as the cache shares them.
    fn pcm(bytes: usize) -> Arc<Pcm> {
        Arc::new(decoded(bytes))
    }

    #[test]
    fn low_pressure_keeps_the_newest_sounds_within_a_quarter_of_the_budget() {
        let mut bank = bank();
        let sound = LOW_PRESSURE_CACHE_BYTES / 2;
        for name in ["oldest", "older", "newer", "newest"] {
            bank.insert_test_pcm(name, pcm(sound));
        }
        assert_eq!(bank.release_decoded(MemoryPressure::Low), 2 * sound);
        assert_eq!(bank.cache_bytes, LOW_PRESSURE_CACHE_BYTES);
        for (name, kept) in [
            ("oldest", false),
            ("older", false),
            ("newer", true),
            ("newest", true),
        ] {
            assert_eq!(bank.cache.contains_key(name), kept, "{name}");
        }
        assert_eq!(bank.release_decoded(MemoryPressure::Low), 0);
    }

    #[test]
    fn critical_pressure_empties_the_cache_but_keeps_running_decodes() {
        let mut bank = bank();
        bank.insert_test_pcm("cached", pcm(64));
        bank.in_flight.insert("decoding".into(), (false, 16));
        bank.in_flight_bytes = 16;
        bank.ready_streams.insert("stream".into(), pcm(64));
        let generation = bank.decode_generation;
        assert_eq!(bank.release_decoded(MemoryPressure::Critical), 64);
        assert!(bank.cache.is_empty() && bank.cache_order.is_empty());
        assert_eq!(bank.cache_bytes, 0);
        assert!(bank.is_decoding("decoding"));
        assert_eq!(bank.ready_streams.len(), 1);
        assert_eq!(bank.decode_generation, generation);
    }

    /// Hidden clients keep nothing decoded, and results of the decodes they abandoned never
    /// reach the cache.
    #[test]
    fn background_pressure_abandons_queued_decodes_and_unclaimed_streams() {
        let mut bank = bank();
        let (jobs, _job_queue) = sync_channel(MAX_QUEUED_DECODES);
        let (results, done) = sync_channel(DECODE_WORKERS);
        let generation = Arc::new(AtomicU64::new(0));
        bank.decoder = Some(Decoder {
            jobs,
            done: Mutex::new(done),
            generation: Arc::clone(&generation),
            encoded_bytes: Arc::new(AtomicUsize::new(0)),
            server: Arc::new(Mutex::new((0, None))),
            external: false,
        });
        bank.insert_test_pcm("sounds/cached", pcm(64));
        bank.in_flight.insert("sounds/decoding".into(), (false, 16));
        bank.in_flight_bytes = 16;
        bank.ready_streams.insert("sounds/stream".into(), pcm(64));

        assert_eq!(bank.release_decoded(MemoryPressure::Background), 64);
        assert!(bank.cache.is_empty() && bank.ready_streams.is_empty());
        assert!(!bank.is_decoding("sounds/decoding"));
        assert_eq!(bank.in_flight_bytes, 0);
        assert_ne!(
            generation.load(Ordering::Acquire),
            0,
            "workers skip the jobs queued before the release"
        );
        results
            .blocking_send((0, "sounds/decoding".into(), Some(decoded(8))))
            .unwrap();
        bank.poll();
        assert!(bank.cache.is_empty());
    }

    /// A released sound is not lost: its next lookup reads and decodes the file again.
    #[test]
    fn a_released_sound_decodes_again_on_its_next_lookup() {
        let bytes = assets::sound_bank::encode_sound_bank(
            b"{}",
            b"{}",
            b"{}",
            &[("sounds/tone".to_owned(), super::super::tests::tone())],
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "cinnabar-pressure-release-{}.mcbesnd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = SoundBank::open(&path, None).unwrap().unwrap();
        let first = bank.pcm("sounds/tone", false).expect("first decode");
        assert!(bank.cache.contains_key("sounds/tone"));

        assert!(bank.release_decoded(MemoryPressure::Background) > 0);
        assert!(bank.cache.is_empty());

        let again = bank
            .pcm("sounds/tone", false)
            .expect("decode after release");
        assert_eq!(again.samples, first.samples);
        assert!(
            !Arc::ptr_eq(&again, &first),
            "the release dropped the cache"
        );
        let _ = std::fs::remove_file(path);
    }
}
