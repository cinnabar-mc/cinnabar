//! The local server command line that serves a built showcase.

use crate::artifact::Built;
use std::path::Path;

/// The address the local server listens on, which is also the audience the server's signed
/// offers name, so a client joining it accepts them.
pub const ADDRESS: &str = "127.0.0.1:19132";

/// The complete local server command for `built`, built into `out` from the workspace at
/// `root`: the binaries `make local-server` writes under `target/release`, a world under `out`,
/// the showcase flags, and one address for `-addr` and `-extension-audience`.
pub fn server_command(root: &Path, out: &Path, built: &Built) -> String {
    let binary = |name: &str| {
        root.join("target/release")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    };
    format!(
        "{} -dir {} -addr {ADDRESS} -material-fx -experiences {} -experience-runtime {} \
         -extension-key {} -extension-audience {ADDRESS} -extension-cxb {}",
        binary("bedrock-local-server").display(),
        out.join("world").display(),
        built.experiences.display(),
        binary("experience-runtime").display(),
        built.server_seed.display(),
        built.cxb_dir.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_printed_command_is_complete_and_has_no_placeholders() {
        let out = Path::new(".local/material-fx");
        let built = Built {
            experiences: out.join("experiences"),
            cxb_dir: out.join("cxb"),
            cxb: out.join("cxb/materialfx.cxb"),
            server_seed: out.join("keys/server.seed"),
        };
        let command = server_command(Path::new("/work"), out, &built);
        let words: Vec<_> = command.split(' ').collect();
        let value = |flag: &str| {
            let at = words.iter().position(|word| *word == flag).unwrap();
            words[at + 1]
        };
        assert!(!command.contains('<'), "{command}");
        assert_eq!(value("-dir"), out.join("world").display().to_string());
        assert_eq!(value("-addr"), value("-extension-audience"));
        assert!(words[0].starts_with("/work/target/release/bedrock-local-server"));
        assert!(
            value("-experience-runtime").starts_with("/work/target/release/experience-runtime")
        );
    }
}
