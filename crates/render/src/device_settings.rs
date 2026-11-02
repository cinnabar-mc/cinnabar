//! Device requirements shared by the native and browser application hosts.

use bevy::render::settings::WgpuSettings;
use std::path::PathBuf;

/// Sub-allocation block sizes for the device's memory allocator: at most 16 MiB, so a block
/// holds about one full-screen target and anything larger gets memory of its own.
const SUBALLOCATED_BLOCK_BYTES: std::ops::Range<u64> = (4 << 20)..(16 << 20);

/// Counts vertex-visible storage bindings directly from a renderer layout.
pub(crate) fn vertex_storage_buffers(
    layout: &bevy::render::render_resource::BindGroupLayoutDescriptor,
) -> u32 {
    storage_buffers(layout, bevy::render::render_resource::ShaderStages::VERTEX)
}

/// Counts storage bindings visible to one stage in a renderer layout.
pub(crate) fn storage_buffers(
    layout: &bevy::render::render_resource::BindGroupLayoutDescriptor,
    stage: bevy::render::render_resource::ShaderStages,
) -> u32 {
    use bevy::render::render_resource::{BindingType, BufferBindingType};
    layout
        .entries
        .iter()
        .filter(|entry| {
            entry.visibility.contains(stage)
                && matches!(
                    entry.ty,
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { .. },
                        ..
                    }
                )
        })
        .count() as u32
}

/// Applies the renderer's storage limits, allocator and label policy and the platform's device
/// policy.
pub fn configure(settings: &mut WgpuSettings) {
    #[cfg(target_arch = "wasm32")]
    {
        settings.priority = bevy::render::settings::WgpuSettingsPriority::Functionality;
        settings.backends = Some(wgpu::Backends::BROWSER_WEBGPU);
    }
    settings.dx12_shader_compiler = dx12_shader_compiler(
        wgpu::Dx12Compiler::from_env(),
        bundled_dxc().filter(|path| path.is_file()),
    );
    settings.limits.max_storage_buffers_per_shader_stage = settings
        .limits
        .max_storage_buffers_per_shader_stage
        .max(crate::required_vertex_storage_buffers());
    // Bevy's default (`Performance`) sub-allocates from 128-256 MiB blocks the allocator
    // keeps while any allocation in them lives, so a block with one live buffer pins the
    // rest of its memory.
    settings.memory_hints = wgpu::MemoryHints::Manual {
        suballocated_device_memory_block_size: SUBALLOCATED_BLOCK_BYTES,
    };
    // Applied after Bevy's environment read, so the environment is read again to keep its override.
    settings.instance_flags = backend_labels(settings.instance_flags).with_env();
}

/// Drops backend object names unless backend debugging or validation is on. A name costs a driver
/// call (DX12 `SetName`, Metal `setLabel`) for every command list, pass encoder and resource,
/// about fifty command lists a frame on DX12, and only graphics debuggers and validation layers
/// read it. `WGPU_DISCARD_HAL_LABELS=0` keeps names for a named capture.
fn backend_labels(flags: wgpu::InstanceFlags) -> wgpu::InstanceFlags {
    if flags.intersects(wgpu::InstanceFlags::DEBUG | wgpu::InstanceFlags::VALIDATION) {
        flags
    } else {
        flags | wgpu::InstanceFlags::DISCARD_HAL_LABELS
    }
}

/// Where Windows installs and development builds place DXC: beside the client executable.
/// `None` off Windows or when the executable's path is unknown.
pub fn bundled_dxc() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let executable = std::env::current_exe().ok()?;
    Some(executable.parent()?.join(dxc_library()))
}

/// The DXC library's file name, from the pin the Windows packagers install.
fn dxc_library() -> String {
    let pin: serde_json::Value =
        serde_json::from_str(include_str!("../../../packaging/windows/dxc.json"))
            .expect("the Windows DXC pin parses");
    pin["library"]
        .as_str()
        .expect("the Windows DXC pin names its library")
        .to_owned()
}

/// DXC compiles shaders much faster than FXC, and DX12 compiles every pipeline afresh at each
/// launch. Only the bundled copy is used: wgpu's and Bevy's defaults load whichever
/// `dxcompiler.dll` the launch directory or DLL search path holds, and an older copy cannot sign
/// DXIL. Without the bundled copy, or with `WGPU_DX12_COMPILER=fxc`, DX12 keeps FXC.
/// [`adapter_requiring_fxc`] overrides this once the adapters are known.
fn dx12_shader_compiler(
    requested: Option<wgpu::Dx12Compiler>,
    bundled: Option<PathBuf>,
) -> wgpu::Dx12Compiler {
    let bundled = bundled.and_then(|path| path.into_os_string().into_string().ok());
    match (requested, bundled) {
        (Some(wgpu::Dx12Compiler::Fxc), _) | (_, None) => wgpu::Dx12Compiler::Fxc,
        (_, Some(dxc_path)) => wgpu::Dx12Compiler::DynamicDxc { dxc_path },
    }
}

/// Intel's PCI vendor ID.
pub(crate) const INTEL_VENDOR_ID: u32 = 0x8086;
/// The first Intel Windows driver branch (`24.x`, 2018) not known to fail on DX12. HD Graphics 520
/// with `23.20.16.4905` faulted in counted GPU-cull draws, drew no opaque terrain with CPU-culled
/// indirect draws, and lost the device building wgpu's indirect-validation pipeline with DXC.
const FIRST_MODERN_INTEL_DX12_DRIVER_MAJOR: u32 = 24;

/// Recognizes a DX12 Intel adapter whose dotted driver version predates the `24.x` branch.
/// An unparsable version, or the `0.0.0.0` wgpu reports when the query fails, is not legacy.
pub(crate) fn legacy_intel_dx12_driver(info: &wgpu::AdapterInfo) -> bool {
    info.backend == wgpu::Backend::Dx12
        && info.vendor == INTEL_VENDOR_ID
        && info
            .driver
            .split('.')
            .next()
            .and_then(|major| major.trim().parse::<u32>().ok())
            .is_some_and(|major| (1..FIRST_MODERN_INTEL_DX12_DRIVER_MAJOR).contains(&major))
}

/// Names the adapter that needs an instance built with `compiler` to be rebuilt with FXC, because
/// its driver loses the device on DXC's output. The driver is only known once an instance has
/// listed its adapters. An instance holds one compiler, so its other adapters take FXC as well.
pub fn adapter_requiring_fxc(
    compiler: &wgpu::Dx12Compiler,
    adapters: impl IntoIterator<Item = wgpu::AdapterInfo>,
) -> Option<wgpu::AdapterInfo> {
    if matches!(compiler, wgpu::Dx12Compiler::Fxc) {
        return None;
    }
    adapters.into_iter().find(legacy_intel_dx12_driver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_sub_allocates_from_blocks_of_at_most_16_mib() {
        let mut settings = WgpuSettings::default();
        configure(&mut settings);
        assert!(
            matches!(
                &settings.memory_hints,
                wgpu::MemoryHints::Manual { suballocated_device_memory_block_size }
                    if suballocated_device_memory_block_size.end <= 16 << 20
            ),
            "{:?}",
            settings.memory_hints
        );
    }

    #[test]
    fn backend_names_are_kept_only_while_backend_debugging_or_validating() {
        use wgpu::InstanceFlags;
        assert_eq!(
            backend_labels(InstanceFlags::empty()),
            InstanceFlags::DISCARD_HAL_LABELS
        );
        assert_eq!(
            backend_labels(InstanceFlags::VALIDATION_INDIRECT_CALL),
            InstanceFlags::VALIDATION_INDIRECT_CALL | InstanceFlags::DISCARD_HAL_LABELS
        );
        for debugging in [
            InstanceFlags::DEBUG,
            InstanceFlags::VALIDATION,
            InstanceFlags::debugging(),
        ] {
            assert_eq!(backend_labels(debugging), debugging);
        }
    }

    /// The reporting laptop's adapters, as its client log prints them.
    fn hd_520(driver: &str) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "Intel(R) HD Graphics 520".into(),
            vendor: INTEL_VENDOR_ID,
            device: 0x1916,
            driver: driver.into(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Dx12)
        }
    }

    #[test]
    fn legacy_intel_drivers_rebuild_a_dxc_instance_with_fxc() {
        let dxc = wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: r"C:\Program Files\Cinnabar\dxcompiler.dll".into(),
        };
        let software = wgpu::AdapterInfo {
            name: "Microsoft Basic Render Driver".into(),
            vendor: 0x1414,
            driver: "10.0.19041.5794".into(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Dx12)
        };
        let legacy = hd_520("23.20.16.4905");
        let found = adapter_requiring_fxc(&dxc, [software.clone(), legacy.clone()]);
        assert_eq!(found.map(|info| info.name), Some(legacy.name.clone()));
        // An instance already on FXC has nothing to rebuild.
        assert!(adapter_requiring_fxc(&wgpu::Dx12Compiler::Fxc, [legacy.clone()]).is_none());
        // Current, unknown and other-backend drivers keep DXC's faster compiles.
        let vulkan = wgpu::AdapterInfo {
            backend: wgpu::Backend::Vulkan,
            ..legacy
        };
        for adapter in [
            software,
            vulkan,
            hd_520("24.20.100.6025"),
            hd_520("31.0.101.2135"),
            hd_520("0.0.0.0"),
            hd_520(""),
        ] {
            let driver = adapter.driver.clone();
            assert!(adapter_requiring_fxc(&dxc, [adapter]).is_none(), "{driver}");
        }
    }

    #[test]
    fn dx12_uses_only_the_bundled_dxc_unless_fxc_is_requested() {
        let bundled = PathBuf::from(r"C:\Program Files\Cinnabar\dxcompiler.dll");
        for requested in [None, Some(wgpu::Dx12Compiler::default_dynamic_dxc())] {
            assert!(matches!(
                dx12_shader_compiler(requested.clone(), Some(bundled.clone())),
                wgpu::Dx12Compiler::DynamicDxc { dxc_path } if std::path::Path::new(&dxc_path) == bundled
            ));
            assert!(matches!(
                dx12_shader_compiler(requested, None),
                wgpu::Dx12Compiler::Fxc
            ));
        }
        assert!(matches!(
            dx12_shader_compiler(Some(wgpu::Dx12Compiler::Fxc), Some(bundled)),
            wgpu::Dx12Compiler::Fxc
        ));
    }

    #[test]
    fn shader_compiler_does_not_inherit_a_launch_directory_dxc() {
        let mut settings = WgpuSettings {
            dx12_shader_compiler: wgpu::Dx12Compiler::default_dynamic_dxc(),
            ..Default::default()
        };
        configure(&mut settings);
        match settings.dx12_shader_compiler {
            wgpu::Dx12Compiler::Fxc => {}
            wgpu::Dx12Compiler::DynamicDxc { dxc_path } => {
                assert_eq!(Some(PathBuf::from(dxc_path)), bundled_dxc());
            }
            other => panic!("unexpected DX12 shader compiler {other:?}"),
        }
    }

    #[test]
    fn pinned_library_is_the_dxc_wgpu_loads() {
        let wgpu::Dx12Compiler::DynamicDxc { dxc_path } = wgpu::Dx12Compiler::default_dynamic_dxc()
        else {
            unreachable!("default_dynamic_dxc builds a dynamic compiler");
        };
        assert_eq!(dxc_library(), dxc_path);
    }

    #[cfg(windows)]
    #[test]
    fn bundled_dxc_sits_beside_the_executable() {
        let executable = std::env::current_exe().unwrap();
        assert_eq!(
            bundled_dxc().as_deref().and_then(std::path::Path::parent),
            executable.parent()
        );
    }
}
