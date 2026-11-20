//! Persists the chosen GPU adapter beside the client log. Android's crash dialog and saved
//! report read the file, so a report names the GPU and driver even when the log's startup lines
//! are lost or the process dies before logging them.

use std::path::{Path, PathBuf};

use bevy::{
    app::{App, Plugin},
    render::renderer::RenderAdapterInfo,
};

/// The record's file name inside the install layout's log directory. Android's packaging
/// manifest names the resulting path for the crash dialog.
pub(crate) const FILE_NAME: &str = "adapter.json";

/// Writes the record once the renderer has chosen its adapter, before the first frame.
pub(crate) struct AdapterRecord {
    /// The install layout's log directory.
    pub(crate) log_dir: PathBuf,
}

impl Plugin for AdapterRecord {
    fn build(&self, _app: &mut App) {}

    /// Runs after the render plugin's `finish`, which publishes the adapter; add this plugin
    /// after the renderer.
    fn finish(&self, app: &mut App) {
        let Some(info) = app.world().get_resource::<RenderAdapterInfo>() else {
            return;
        };
        if let Err(error) = write(&self.log_dir, info) {
            bevy::log::warn!("GPU adapter record not written: {error}");
        }
    }
}

/// Replaces the record in `log_dir` with `info` and this session's identity.
fn write(log_dir: &Path, info: &wgpu::AdapterInfo) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let record = record(
        info,
        diagnostics::session::id(),
        diagnostics::session::COMMIT,
    );
    std::fs::write(log_dir.join(FILE_NAME), record)
}

/// The record's fields, in the order the crash dialog shows them.
#[derive(serde::Serialize)]
struct Record<'a> {
    name: &'a str,
    /// Hexadecimal, as drivers and device databases list ids.
    vendor: String,
    device: String,
    device_type: String,
    driver: &'a str,
    driver_info: &'a str,
    backend: &'a str,
    session: &'a str,
    commit: &'a str,
}

/// The record as indented JSON, which the crash dialog shows verbatim: the adapter, its
/// driver's name and version text and the backend, with the session and commit that wrote it.
/// It names no account, server or player.
fn record(info: &wgpu::AdapterInfo, session: &str, commit: &str) -> String {
    let record = Record {
        name: &info.name,
        vendor: format!("{:#06x}", info.vendor),
        device: format!("{:#06x}", info.device),
        device_type: format!("{:?}", info.device_type),
        driver: &info.driver,
        driver_info: &info.driver_info,
        backend: info.backend.to_str(),
        session,
        commit,
    };
    let mut text = serde_json::to_string_pretty(&record)
        .expect("a record of strings serializes without failure");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Qualcomm-style adapter, the kind crash reports most often fail to name.
    fn adapter() -> wgpu::AdapterInfo {
        let mut info =
            wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Vulkan);
        info.name = "Adreno (TM) 710".into();
        info.vendor = 0x5143;
        info.device = 0x4305_1401;
        info.driver = "Qualcomm Technologies Inc. Adreno Vulkan Driver".into();
        info.driver_info = "Driver Build: b82916be04, \"quoted\"".into();
        info
    }

    /// The crash report identifies the GPU, driver and writing build from this file alone.
    #[test]
    fn record_names_the_adapter_driver_and_session() {
        let text = record(&adapter(), "0123456789abcdef", "ba9516d01");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let expected = serde_json::json!({
            "name": "Adreno (TM) 710",
            "vendor": "0x5143",
            "device": "0x43051401",
            "device_type": "IntegratedGpu",
            "driver": "Qualcomm Technologies Inc. Adreno Vulkan Driver",
            "driver_info": "Driver Build: b82916be04, \"quoted\"",
            "backend": "vulkan",
            "session": "0123456789abcdef",
            "commit": "ba9516d01",
        });
        assert_eq!(value, expected);
        // The dialog shows the file as written: one field per line, the adapter's name first.
        assert_eq!(text.lines().nth(1), Some(r#"  "name": "Adreno (TM) 710","#));
    }

    /// The plugin writes during `finish`, so the record exists before the first frame, and a
    /// later session replaces it.
    #[test]
    fn plugin_writes_the_record_before_the_first_update() {
        let root = tempfile::tempdir().unwrap();
        let log_dir = root.path().join("logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(log_dir.join(FILE_NAME), "an earlier session").unwrap();
        let mut app = App::new();
        app.insert_resource(RenderAdapterInfo::new(adapter()))
            .add_plugins(AdapterRecord {
                log_dir: log_dir.clone(),
            });
        app.finish();
        let text = std::fs::read_to_string(log_dir.join(FILE_NAME)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["name"], "Adreno (TM) 710");
        assert_eq!(value["session"], diagnostics::session::id());
    }

    /// Without a renderer there is no adapter to record and nothing is written.
    #[test]
    fn no_adapter_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(AdapterRecord {
            log_dir: root.path().to_owned(),
        });
        app.finish();
        assert!(!root.path().join(FILE_NAME).exists());
    }

    /// The crash dialog is Java and reads the record at the path the Android packaging
    /// manifest names, so the client must write it there.
    #[test]
    fn android_packaging_names_the_recorded_path() {
        let files = Path::new("/files");
        let layout = launcher::install_layout::InstallLayout::android(
            files,
            PathBuf::from("/lib/core"),
            PathBuf::from("/lib/worker"),
        );
        let path = layout.log_dir().join(FILE_NAME);
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../packaging/android/runtime.json")).unwrap();
        assert_eq!(
            manifest["adapter_record"].as_str().map(Path::new),
            path.strip_prefix(files).ok()
        );
    }
}
