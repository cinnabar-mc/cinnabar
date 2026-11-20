//! The device-code prompt the sign-in helper saves beside a token cache while the player approves
//! it in the browser (`core/authcache/pending_prompt.go`). Phones end apps left for the browser;
//! the next sign-in resumes a prompt that is still valid instead of losing it. Only the helper
//! writes or removes it; the client reads which code a new helper will resume.
//!
//! The client keeps its own note beside it: the code its player was sent to the browser with and
//! has not cancelled. A launch that finds both resumes that sign-in unasked. The note is the
//! client's alone, so a cancelled sign-in stays closed without touching the helper's file.

use std::{
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const SUFFIX: &str = include_str!("../../../core/authcache/pending_prompt_suffix.txt");
/// Names the client's note of the code its player is waiting on, beside the token cache.
const WAITING_SUFFIX: &str = ".sign-in-waiting-v1";
/// Bounds the note, which holds one device code.
const MAX_WAITING_BYTES: u64 = 64;
/// Keeps concurrent notes from sharing a temporary file.
static TEMPORARY: AtomicU64 = AtomicU64::new(0);
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

/// Reads at most `limit` bytes of `path`; missing, unreadable and longer files count as none.
fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut contents = Vec::new();
    if file.take(limit + 1).read_to_end(&mut contents).is_err() || contents.len() as u64 > limit {
        return None;
    }
    Some(contents)
}

/// Reads the prompt saved for `cache`; missing and unreadable prompts count as none.
fn read(cache: &Path) -> Option<SavedPrompt> {
    serde_json::from_slice(&read_bounded(&path(cache), MAX_BYTES)?).ok()
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

/// What became of the sign-in a client noted its player was waiting on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LeftWaiting {
    /// The noted code still awaits approval, so a new helper resumes polling it.
    Awaiting(String),
    /// The noted code is gone: approved while the client was away, expired, or replaced.
    Settled,
}

/// Where the client notes the code its player is waiting on for the token cache at `cache`.
pub fn waiting_path(cache: &Path) -> PathBuf {
    let mut name = cache.as_os_str().to_owned();
    name.push(WAITING_SUFFIX);
    PathBuf::from(name)
}

/// Notes that this client's player is waiting on `user_code` for `cache`, before they leave for
/// the browser. The note survives the system ending the client; it is not synced to storage,
/// since only the client's own process ending has to keep it.
pub fn note_waiting(cache: &Path, user_code: &str) -> io::Result<()> {
    let note = waiting_path(cache);
    let parent = note
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "token cache has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".sign-in-waiting-{}-{}.tmp",
        std::process::id(),
        TEMPORARY.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&temporary)?.write_all(user_code.as_bytes())?;
        fs::rename(&temporary, &note)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Removes the client's note for `cache`, once its sign-in finished, failed or was cancelled.
pub fn forget_waiting(cache: &Path) -> io::Result<()> {
    match fs::remove_file(waiting_path(cache)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// What became, by `now`, of the sign-in a client noted for `cache`; `None` when no client left
/// one waiting. Only the noted code resumes: a prompt saved for another attempt, or by another
/// client sharing the install, is left for an explicit sign-in.
pub fn left_waiting(cache: &Path, now: SystemTime) -> Option<LeftWaiting> {
    let noted = String::from_utf8(read_bounded(&waiting_path(cache), MAX_WAITING_BYTES)?).ok();
    Some(match (noted, awaiting(cache, now)) {
        (Some(noted), Some(saved)) if noted == saved => LeftWaiting::Awaiting(saved),
        _ => LeftWaiting::Settled,
    })
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

    #[test]
    fn only_the_noted_code_is_left_waiting_and_only_until_it_settles() {
        let directory = crate::test_support::scratch("sign-in-waiting").user_data_root;
        let cache = directory.join("auth/microsoft-token.json");
        let before_expiry = UNIX_EPOCH + Duration::from_secs(FIXTURE_EXPIRY - 1);
        let at_expiry = UNIX_EPOCH + Duration::from_secs(FIXTURE_EXPIRY);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(path(&cache), FIXTURE).unwrap();
        assert_eq!(
            left_waiting(&cache, before_expiry),
            None,
            "a prompt nobody noted, as another client's or a cancelled one, is not resumed"
        );

        note_waiting(&cache, "ABCD-1234").unwrap();
        assert_eq!(
            left_waiting(&cache, before_expiry),
            Some(LeftWaiting::Awaiting("ABCD-1234".into()))
        );
        assert_eq!(
            left_waiting(&cache, at_expiry),
            Some(LeftWaiting::Settled),
            "the noted code expired"
        );

        note_waiting(&cache, "OTHER-CODE").unwrap();
        assert_eq!(
            left_waiting(&cache, before_expiry),
            Some(LeftWaiting::Settled),
            "the saved prompt belongs to another attempt"
        );
        note_waiting(&cache, "ABCD-1234").unwrap();
        fs::remove_file(path(&cache)).unwrap();
        assert_eq!(
            left_waiting(&cache, before_expiry),
            Some(LeftWaiting::Settled),
            "the helper removed the approved code"
        );

        forget_waiting(&cache).unwrap();
        forget_waiting(&cache).expect("forgetting twice is harmless");
        assert_eq!(left_waiting(&cache, before_expiry), None);
        assert!(
            fs::read_dir(cache.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")),
            "a note left a temporary file behind"
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
