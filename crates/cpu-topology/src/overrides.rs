//! Diagnostic overrides for the CPU plan.

/// Diagnostic cap on the logical processors every thread pool sizes itself for.
pub(crate) const CORES_ENV: &str = "CINNABAR_CPU_CORES";
/// `0` ignores the core topology; `1` also applies it on desktop Linux. Unset applies it on
/// Android only.
pub(crate) const TOPOLOGY_ENV: &str = "CINNABAR_CPU_TOPOLOGY";

/// An override's value from the environment or, on Android, where an installed app gets no
/// environment, from the matching `debug.` system property, such as
/// `debug.cinnabar.cpu_topology`.
pub(crate) fn value(name: &str) -> Option<String> {
    let value = std::env::var(name).ok();
    #[cfg(target_os = "android")]
    let value = value.or_else(|| system_property(&android_property(name)?));
    value
}

/// Applies a positive integer cap below `available`; anything else keeps `available`.
pub(crate) fn capped_cores(available: usize, requested: Option<&str>) -> usize {
    requested
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&cores| cores > 0)
        .map_or(available, |cores| cores.min(available))
}

/// The `debug.` system property that mirrors a `CINNABAR_` override; `None` without the prefix.
#[cfg(any(target_os = "android", test))]
pub(crate) fn android_property(name: &str) -> Option<String> {
    let rest = name.strip_prefix("CINNABAR_")?;
    Some(format!("debug.cinnabar.{}", rest.to_ascii_lowercase()))
}

/// Reads an Android system property; `None` when unset, empty or not UTF-8.
#[cfg(target_os = "android")]
fn system_property(name: &str) -> Option<String> {
    let name = std::ffi::CString::new(name).ok()?;
    let mut value = [0_u8; libc::PROP_VALUE_MAX as usize];
    // SAFETY: `name` is NUL-terminated and `value` holds PROP_VALUE_MAX bytes, as required.
    let length = unsafe { libc::__system_property_get(name.as_ptr(), value.as_mut_ptr().cast()) };
    let length = usize::try_from(length).ok().filter(|&length| length > 0)?;
    String::from_utf8(value[..length.min(value.len())].to_vec()).ok()
}
