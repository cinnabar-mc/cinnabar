//! The device-code prompt the sign-in helper saves beside a token cache while the player approves
//! it in the browser (`core/authcache/pending_prompt.go`). Phones end apps left for the browser;
//! the next sign-in resumes a prompt that is still valid instead of losing it. Only the helper
//! writes or removes it; the client reads which code a new helper will resume.

use std::{
    fs,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The helper's own encoding, shared with `core/authcache`'s conformance test.
    const FIXTURE: &str = include_str!("../../../core/authcache/testdata/pending_prompt.json");
    const FIXTURE_EXPIRY: u64 = 1_893_456_000;

    #[test]
    fn a_saved_prompt_awaits_approval_until_it_expires() {
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
        fs::write(path(&cache), b"{not json").unwrap();
        assert_eq!(
            awaiting(&cache, at(0)),
            None,
            "an unreadable prompt is not resumed"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
