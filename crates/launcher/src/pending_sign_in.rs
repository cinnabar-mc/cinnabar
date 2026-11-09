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

/// The part of the saved prompt the client reads; the code itself stays with the helper.
#[derive(serde::Deserialize)]
struct SavedPrompt {
    /// Unix seconds after which the provider no longer accepts the code.
    expires_at: i64,
}

/// Where the helper saves the prompt awaiting approval for the token cache at `cache`.
pub fn path(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(SUFFIX);
    PathBuf::from(name)
}

/// Whether a prompt saved for `cache` still awaits approval at `now`. Missing, unreadable and
/// expired prompts count as none; the helper removes them when it next runs.
pub fn awaiting(cache: &Path, now: SystemTime) -> bool {
    let Ok(file) = fs::File::open(path(cache)) else {
        return false;
    };
    let mut contents = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut contents).is_err()
        || contents.len() as u64 > MAX_BYTES
    {
        return false;
    }
    let Ok(saved) = serde_json::from_slice::<SavedPrompt>(&contents) else {
        return false;
    };
    let now = now.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    });
    now < saved.expires_at
}

/// Removes the prompt saved for `cache`, so a sign-in the player abandoned is not reopened.
pub fn forget(cache: &Path) -> io::Result<()> {
    match fs::remove_file(path(cache)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The helper's own encoding, shared with `core/authcache`'s conformance test.
    const FIXTURE: &str = include_str!("../../../core/authcache/testdata/pending_prompt.json");
    const FIXTURE_EXPIRY: u64 = 1_893_456_000;

    #[test]
    fn a_saved_prompt_awaits_approval_until_it_expires_and_forgetting_removes_it() {
        let directory = crate::test_support::scratch("pending-sign-in").user_data_root;
        let cache = directory.join("microsoft-token.json");
        fs::create_dir_all(&directory).unwrap();
        let at = |seconds| UNIX_EPOCH + Duration::from_secs(seconds);
        assert!(!awaiting(&cache, at(0)), "no prompt was saved");
        fs::write(path(&cache), FIXTURE).unwrap();
        assert!(awaiting(&cache, at(FIXTURE_EXPIRY - 1)));
        assert!(
            !awaiting(&cache, at(FIXTURE_EXPIRY)),
            "the code has expired"
        );
        forget(&cache).unwrap();
        assert!(!path(&cache).exists());
        forget(&cache).expect("forgetting twice is harmless");
        fs::write(path(&cache), b"{not json").unwrap();
        assert!(
            !awaiting(&cache, at(0)),
            "an unreadable prompt is not reopened"
        );
    }
}
