//! The device-code prompt the sign-in helper saves beside a token cache while the player approves
//! it in the browser (`core/authcache/pending_prompt.go`). Phones end apps left for the browser;
//! the next launch reopens a prompt that is still valid instead of losing the sign-in.

use std::{
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const SUFFIX: &str = include_str!("../../../core/authcache/pending_prompt_suffix.txt");
/// Matches the helper's own bound on a saved prompt.
const MAX_BYTES: u64 = 8 * 1024;

/// The part of the saved prompt the client reads; the device code itself stays with the helper.
#[derive(serde::Deserialize)]
struct SavedPrompt {
    /// The code the player enters in the browser, as the prompt showed it.
    user_code: String,
    /// Unix seconds after which the provider no longer accepts the code.
    expires_at: i64,
    /// The tag of the client whose helper last saved the prompt.
    #[serde(default)]
    owner: String,
}

/// Where the helper saves the prompt awaiting approval for the token cache at `cache`.
pub fn path(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(SUFFIX);
    PathBuf::from(name)
}

/// Reads the prompt saved for `cache`; missing and unreadable prompts count as none.
fn read(cache: &Path) -> Option<SavedPrompt> {
    let file = fs::File::open(path(cache)).ok()?;
    let mut contents = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut contents).is_err()
        || contents.len() as u64 > MAX_BYTES
    {
        return None;
    }
    serde_json::from_slice(&contents).ok()
}

/// The user code of a prompt saved for `cache` that still awaits approval at `now`. Missing,
/// unreadable and expired prompts count as none; the helper removes them when it next runs.
pub fn awaiting(cache: &Path, now: SystemTime) -> Option<String> {
    let saved = read(cache)?;
    let now = now.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    });
    (now < saved.expires_at).then_some(saved.user_code)
}

/// Removes the prompt saved for `cache`, so a sign-in the player abandoned is not reopened.
pub fn forget(cache: &Path) -> io::Result<()> {
    match fs::remove_file(path(cache)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Removes the prompt saved for `cache` only if the helper tagged `owner` saved it last. Another
/// client sharing the install keeps a prompt its own helper saved or took over.
pub fn forget_owned(cache: &Path, owner: &str) -> io::Result<()> {
    match read(cache) {
        Some(saved) if saved.owner == owner => forget(cache),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The helper's own encoding, shared with `core/authcache`'s conformance test.
    const FIXTURE: &str = include_str!("../../../core/authcache/testdata/pending_prompt.json");
    const FIXTURE_EXPIRY: u64 = 1_893_456_000;
    const FIXTURE_OWNER: &str = "client-1";

    #[test]
    fn a_saved_prompt_awaits_approval_until_it_expires_and_forgetting_removes_it() {
        let directory = crate::test_support::scratch("pending-sign-in").user_data_root;
        let cache = directory.join("microsoft-token.json");
        fs::create_dir_all(&directory).unwrap();
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(awaiting(&cache, at(0)), None, "no prompt was saved");
        fs::write(path(&cache), FIXTURE).unwrap();
        assert_eq!(
            awaiting(&cache, at(FIXTURE_EXPIRY - 1)).as_deref(),
            Some("ABCD-1234")
        );
        assert_eq!(
            awaiting(&cache, at(FIXTURE_EXPIRY)),
            None,
            "the code has expired"
        );
        forget(&cache).unwrap();
        assert!(!path(&cache).exists());
        forget(&cache).expect("forgetting twice is harmless");
        fs::write(path(&cache), b"{not json").unwrap();
        assert_eq!(
            awaiting(&cache, at(0)),
            None,
            "an unreadable prompt is not reopened"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn only_the_owning_client_forgets_a_saved_prompt() {
        let directory = crate::test_support::scratch("pending-sign-in-owner").user_data_root;
        let cache = directory.join("microsoft-token.json");
        fs::create_dir_all(&directory).unwrap();
        fs::write(path(&cache), FIXTURE).unwrap();
        forget_owned(&cache, "another-client").unwrap();
        assert!(path(&cache).exists(), "another client's prompt was removed");
        forget_owned(&cache, FIXTURE_OWNER).unwrap();
        assert!(!path(&cache).exists());
        forget_owned(&cache, FIXTURE_OWNER).expect("a missing prompt is already forgotten");
        fs::remove_dir_all(directory).unwrap();
    }
}
