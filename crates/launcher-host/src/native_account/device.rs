//! Hardware model discovery at the native host edge, without hostnames or serial numbers.

/// Reports current hardware claims while the install storage owner retains its stable identifier.
pub(super) fn profile() -> protocol::NativeDeviceProfile {
    protocol::NativeDeviceProfile::for_host(model().as_deref().unwrap_or_default())
}

/// Uses the firmware's public product name when present.
#[cfg(target_os = "linux")]
fn model() -> Option<String> {
    let value = std::fs::read_to_string("/sys/class/dmi/id/product_name").ok()?;
    let value = value.trim();
    (!matches!(value, "To Be Filled By O.E.M." | "Default string")).then(|| value.to_owned())
}

/// Reads the same public Android product property as getprop without launching a process.
#[cfg(target_os = "android")]
fn model() -> Option<String> {
    let mut buffer = [0u8; libc::PROP_VALUE_MAX as usize];
    // SAFETY: the property name is NUL-terminated and Android requires PROP_VALUE_MAX writable bytes.
    let length = unsafe {
        libc::__system_property_get(c"ro.product.model".as_ptr(), buffer.as_mut_ptr().cast())
    };
    let length = usize::try_from(length).ok()?;
    String::from_utf8(buffer.get(..length)?.to_vec()).ok()
}

/// Reads Apple's public hardware model identifier from the kernel.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn model() -> Option<String> {
    let property = if cfg!(target_os = "ios") {
        c"hw.machine"
    } else {
        c"hw.model"
    };
    let mut length = 0;
    // SAFETY: the name is NUL-terminated; a null output requests only the required byte count.
    if unsafe {
        libc::sysctlbyname(
            property.as_ptr(),
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let mut buffer = vec![0u8; length];
    // SAFETY: the output allocation has the exact byte length supplied to sysctlbyname.
    if unsafe {
        libc::sysctlbyname(
            property.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    let bytes = buffer
        .get(..length)?
        .strip_suffix(&[0])
        .unwrap_or(&buffer[..length]);
    String::from_utf8(bytes.to_vec()).ok()
}

/// Reads the firmware product name without collecting registry identifiers.
#[cfg(target_os = "windows")]
fn model() -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let key: Vec<u16> = "HARDWARE\\DESCRIPTION\\System\\BIOS\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "SystemProductName\0".encode_utf16().collect();
    let mut length = 0u32;
    // SAFETY: both names are NUL-terminated; a null output requests the registry value size.
    if unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut length,
        )
    } != 0
    {
        return None;
    }
    let mut buffer = vec![0u16; usize::try_from(length).ok()?.div_ceil(size_of::<u16>())];
    // SAFETY: the output allocation covers the full byte length supplied to RegGetValueW.
    if unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut length,
        )
    } != 0
    {
        return None;
    }
    let units = buffer.get(..usize::try_from(length).ok()? / size_of::<u16>())?;
    String::from_utf16(units.strip_suffix(&[0]).unwrap_or(units)).ok()
}

/// Leaves unavailable hardware unreported instead of substituting an invented model.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
fn model() -> Option<String> {
    None
}
