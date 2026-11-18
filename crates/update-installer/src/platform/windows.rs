//! Windows Installer owns update writes, permission consent and rollback.
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::Command,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0},
    System::{
        ApplicationInstallationAndServicing::MsiEnumRelatedProductsW,
        Registry::{
            HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY,
            KEY_WOW64_64KEY, RRF_RT_REG_MULTI_SZ, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
            RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
        },
        Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject},
    },
    UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
};

/// Opens a verified, write-locked MSI and runs the system installer with no elevation verb.
pub(super) fn install(
    artifact: &Path,
    work: &Path,
    target: &Path,
    expected: &update_manifest::Artifact,
) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let msi = work.join("update.msi");
    std::fs::copy(artifact, &msi)?;
    let _locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&msi)?;
    crate::stage::verify_file(&msi, expected)?;
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(msiexec()?);
    for argument in super::msiexec_arguments(&msi, target) {
        command.raw_arg(argument);
    }
    let status = command.status()?;
    ensure!(
        matches!(status.code(), Some(0 | 3010)),
        "Windows Installer failed: {status}"
    );
    Ok(())
}

/// The system msiexec, never one found through the search path.
fn msiexec() -> Result<PathBuf> {
    let mut directory = [0u16; 32768];
    // SAFETY: the API receives a valid writable buffer and its length.
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            directory.as_mut_ptr(),
            directory.len() as u32,
        )
    } as usize;
    ensure!(
        count > 0 && count < directory.len(),
        "cannot locate Windows Installer"
    );
    Ok(PathBuf::from(OsString::from_wide(&directory[..count])).join("msiexec.exe"))
}

/// Product codes of installed per-machine MSIs from before per-user packages.
pub(super) fn machine_products() -> Result<Vec<OsString>> {
    let install = update_manifest::platforms::windows_install();
    let code = wide(format!("{{{}}}", install.machine_upgrade_code));
    let mut products = Vec::new();
    for index in 0u32.. {
        let mut product = [0u16; 39];
        // SAFETY: the upgrade code is NUL-terminated and the buffer holds a braced GUID and NUL.
        match unsafe { MsiEnumRelatedProductsW(code.as_ptr(), 0, index, product.as_mut_ptr()) } {
            ERROR_SUCCESS => products.push(OsString::from_wide(until_nul(&product))),
            ERROR_NO_MORE_ITEMS => break,
            status => bail!("cannot list per-machine installs: error {status}"),
        }
    }
    Ok(products)
}

/// The folder the per-user MSI recorded for this user, if one is installed.
pub(super) fn recorded_user_folder() -> Option<PathBuf> {
    let install = update_manifest::platforms::windows_install();
    let folder = value(
        HKEY_CURRENT_USER,
        &wide(&install.user_key),
        &install.user_folder_value,
        RRF_RT_REG_SZ,
    )?;
    Some(PathBuf::from(OsString::from_wide(until_nul(&folder)))).filter(|path| path.is_absolute())
}

/// Runs this helper elevated in `flag` mode; Windows asks the user, who may decline.
pub(super) fn run_elevated(flag: &str) -> Result<()> {
    let helper = wide(std::env::current_exe()?);
    let (verb, parameters) = (wide("runas"), wide(flag));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: helper.as_ptr(),
        lpParameters: parameters.as_ptr(),
        ..Default::default()
    };
    // SAFETY: every string outlives the call, which fills `hProcess` on success.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(std::io::Error::last_os_error()).context("administrator approval");
    }
    ensure!(!info.hProcess.is_null(), "elevated helper did not start");
    let process = Process(info.hProcess);
    // SAFETY: the handle stays open until `process` drops.
    let waited = unsafe { WaitForSingleObject(process.0, INFINITE) };
    ensure!(
        waited == WAIT_OBJECT_0,
        "cannot wait for the elevated helper"
    );
    let mut code = 0u32;
    // SAFETY: the handle stays open until `process` drops, and `code` is writable.
    ensure!(
        unsafe { GetExitCodeProcess(process.0, &mut code) } != 0,
        "cannot read the elevated helper's exit code"
    );
    ensure!(code == 0, "elevated helper failed: exit code {code}");
    Ok(())
}

/// Removes the per-machine setup bundles first, since each removes its own MSI, then any MSI
/// installed without one or upgraded outside its bundle. Requires administrator rights.
pub(super) fn remove_machine_install() -> Result<()> {
    let install = update_manifest::platforms::windows_install();
    for bundle in machine_bundles(&install.machine_bundle_upgrade_code) {
        let status = Command::new(&bundle)
            .args(["/uninstall", "/quiet", "/norestart"])
            .status()
            .with_context(|| format!("run {}", bundle.display()))?;
        ensure!(
            matches!(status.code(), Some(0 | 3010)),
            "per-machine setup removal failed: {status}"
        );
    }
    for product in machine_products()? {
        let status = Command::new(msiexec()?)
            .arg("/x")
            .arg(&product)
            .args([
                "/quiet",
                "/norestart",
                "REBOOT=ReallySuppress",
                "MSIDISABLERMRESTART=1",
            ])
            .status()?;
        // 1605: the product is already gone.
        ensure!(
            matches!(status.code(), Some(0 | 1605 | 3010)),
            "per-machine install removal failed: {status}"
        );
    }
    Ok(())
}

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// Cached setup executables of bundles registered with `upgrade_code`, from either registry view.
fn machine_bundles(upgrade_code: &str) -> Vec<PathBuf> {
    let wanted = format!("{{{upgrade_code}}}");
    let uninstall = wide(UNINSTALL);
    let mut bundles = Vec::new();
    for (view, subkey_view) in [
        (KEY_WOW64_64KEY, RRF_SUBKEY_WOW6464KEY),
        (KEY_WOW64_32KEY, RRF_SUBKEY_WOW6432KEY),
    ] {
        let mut key = std::ptr::null_mut();
        // SAFETY: the path is NUL-terminated and `key` receives a handle that `Key` closes.
        if unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                uninstall.as_ptr(),
                0,
                KEY_READ | view,
                &mut key,
            )
        } != ERROR_SUCCESS
        {
            continue;
        }
        let key = Key(key);
        for index in 0u32.. {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            // SAFETY: `name` holds `length` characters, and the optional outputs are null.
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            let subkey = &name[..=length as usize];
            let related = value(
                key.0,
                subkey,
                "BundleUpgradeCode",
                RRF_RT_REG_MULTI_SZ | subkey_view,
            )
            .is_some_and(|codes| {
                codes
                    .split(|&unit| unit == 0)
                    .any(|code| String::from_utf16_lossy(code).eq_ignore_ascii_case(&wanted))
            });
            if related
                && let Some(path) = value(
                    key.0,
                    subkey,
                    "BundleCachePath",
                    RRF_RT_REG_SZ | subkey_view,
                )
            {
                bundles.push(PathBuf::from(OsString::from_wide(until_nul(&path))));
            }
        }
    }
    bundles.sort();
    bundles.dedup();
    bundles
}

/// A string or multi-string value of `key\subkey`, or `None` when it is absent or changing.
fn value(key: HKEY, subkey: &[u16], name: &str, flags: u32) -> Option<Vec<u16>> {
    let name = wide(name);
    let mut size = 0u32;
    // SAFETY: both strings are NUL-terminated; a null buffer asks only for the size in bytes.
    let status = unsafe {
        RegGetValueW(
            key,
            subkey.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `data` holds at least `size` bytes.
    let status = unsafe {
        RegGetValueW(
            key,
            subkey.as_ptr(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS).then(|| {
        data.truncate(size as usize / 2);
        data
    })
}

fn until_nul(units: &[u16]) -> &[u16] {
    &units[..units
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(units.len())]
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

struct Key(HKEY);

impl Drop for Key {
    /// Closes the registry handle on every path.
    fn drop(&mut self) {
        // SAFETY: closes the handle owned by this wrapper.
        unsafe { RegCloseKey(self.0) };
    }
}

struct Process(HANDLE);

impl Drop for Process {
    /// Closes the process handle on every path.
    fn drop(&mut self) {
        // SAFETY: closes the handle owned by this wrapper.
        unsafe { CloseHandle(self.0) };
    }
}
