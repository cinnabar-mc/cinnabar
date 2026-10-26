//! Hash-checks locally persisted driver data and replaces it atomically.

use bevy::render::renderer::RenderDevice;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const DIGEST_BYTES: usize = 32;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub(super) struct DriverCache {
    pub pipeline: wgpu::PipelineCache,
    path: PathBuf,
}

impl DriverCache {
    /// Loads data produced by wgpu for this adapter, falling back on a missing or corrupt file.
    pub(super) fn open(
        device: &RenderDevice,
        info: &wgpu::AdapterInfo,
        directory: PathBuf,
    ) -> Option<Self> {
        let key = wgpu::util::pipeline_cache_key(info)?;
        let path = directory.join(format!("{key}.bin"));
        let data = std::fs::read(&path).ok().and_then(decode);
        // SAFETY: Only hash-checked bytes written from PipelineCache::get_data are supplied.
        // wgpu checks its format, adapter and driver identity; fallback discards stale data.
        let pipeline = unsafe {
            device
                .wgpu_device()
                .create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                    label: Some("world pipeline cache"),
                    data: data.as_deref(),
                    fallback: true,
                })
        };
        Some(Self { pipeline, path })
    }

    /// Reads driver data and writes it away from both application frame threads.
    pub(super) fn store(&self) {
        let Some(data) = self.pipeline.get_data() else {
            return;
        };
        if let Err(error) = write(&self.path, &data) {
            bevy::log::debug!(%error, "optional driver cache not saved");
        }
    }
}

/// Accepts only a complete payload with the digest saved alongside it.
fn decode(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let (digest, data) = bytes.split_at_checked(DIGEST_BYTES)?;
    (Sha256::digest(data).as_slice() == digest).then(|| data.to_vec())
}

/// Atomically replaces one cache file; concurrent clients use distinct temporary paths.
fn write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut bytes = Vec::with_capacity(DIGEST_BYTES + data.len());
    bytes.extend_from_slice(&Sha256::digest(data));
    bytes.extend_from_slice(data);
    let result = std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_and_corrupt_cache_data_falls_back() {
        assert!(decode(vec![0; DIGEST_BYTES - 1]).is_none());
        let payload = b"driver cache";
        let mut bytes = Sha256::digest(payload).to_vec();
        bytes.extend_from_slice(payload);
        assert_eq!(decode(bytes.clone()).as_deref(), Some(payload.as_slice()));
        *bytes.last_mut().unwrap() ^= 1;
        assert!(decode(bytes).is_none());
    }

    #[test]
    fn cache_replacement_is_complete_and_leaves_no_temporary_file() {
        let directory = std::env::temp_dir().join(format!(
            "cinnabar-pipelines-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = directory.join("driver.bin");
        write(&path, b"old").unwrap();
        write(&path, b"replacement").unwrap();
        assert_eq!(
            decode(std::fs::read(&path).unwrap()).as_deref(),
            Some(b"replacement".as_slice())
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
