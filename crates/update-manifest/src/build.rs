//! Installed executable identity queried before an offline helper replaces it.

use crate::channel::Channel;
use serde::{Deserialize, Serialize};

/// Prints build identity before client setup, asset loading or window creation.
pub const QUERY_FLAG: &str = "--update-build-info";

/// App version and publication number reported by the executable being inspected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Build {
    pub version: String,
    pub number: u64,
    /// Older clients and development builds do not report a publication channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,
}

impl From<&crate::Manifest> for Build {
    /// Reads a release identity without duplicating the independently published manifest format.
    fn from(manifest: &crate::Manifest) -> Self {
        Self {
            version: manifest.version.clone(),
            number: manifest.build_number,
            channel: Channel::parse(&manifest.channel),
        }
    }
}

impl Build {
    /// Reports the workspace version and publication number compiled into this executable.
    pub fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            number: crate::build_number(),
            channel: option_env!("CINNABAR_RELEASE_CHANNEL").and_then(Channel::parse),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The helper can still query clients shipped before channel identity was reported.
    #[test]
    fn old_build_queries_remain_compatible_with_channel_identity() {
        let older: Build = serde_json::from_str(r#"{"version":"1.0.0","number":7}"#).unwrap();
        assert_eq!(older.channel, None);
        let mut current = older;
        current.channel = Some(Channel::Nightly);
        let encoded = serde_json::to_string(&current).unwrap();
        assert_eq!(serde_json::from_str::<Build>(&encoded).unwrap(), current);
    }
}
