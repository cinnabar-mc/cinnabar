//! Exact identities for reviewed vocabulary exceptions shared by data validation and compilation.

use serde::{Deserialize, de::Error as _};

const SCHEMA: u32 = 2;

/// One identifier occurrence, scoped to its source file, field and vocabulary domain.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub file: String,
    pub location: Location,
    pub domain: String,
    pub identifier: String,
}

/// File pointers retain exact positions; named policy rules use identifier-keyed fields.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Location {
    Pointer(String),
    Rule {
        collection: String,
        id: String,
        field: String,
    },
}

/// A narrowly scoped exception and the reason the otherwise invalid occurrence is retained.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub reference: Reference,
    pub reason: String,
}

/// The canonical vocabulary exception document used by every consumer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exceptions {
    schema: u32,
    pub entries: Vec<Exception>,
}

impl Exceptions {
    /// Reads the current strict schema and rejects unsupported document versions.
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let document: Self = serde_json::from_slice(bytes)?;
        if document.schema != SCHEMA {
            return Err(serde_json::Error::custom(format!(
                "unsupported vocabulary exceptions schema {}; expected {SCHEMA}",
                document.schema
            )));
        }
        Ok(document)
    }

    /// Finds a reasoned exception for exactly this file, location, domain and identifier.
    pub fn allows(&self, reference: &Reference) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.reference == *reference && !entry.reason.trim().is_empty())
    }
}
