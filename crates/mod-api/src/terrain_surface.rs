//! Retained visual-only block-face policies. Feature choices belong to the caller.
use serde::{Deserialize, Serialize};

pub const MAX_RULES: usize = 64;
pub const MAX_CLAUSES: usize = 32;
pub const MAX_CONDITIONS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub offset: [i8; 3],
    pub present: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clause {
    pub conditions: Vec<Condition>,
    pub tiles: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Independent connection groups sharing this coverage policy.
    pub groups: Vec<Vec<String>>,
    pub border: u8,
    pub clauses: Vec<Clause>,
}

/// Bounds retained work and rejects malformed masks before publication.
pub fn validate(rules: &[Rule]) -> bool {
    rules.len() <= MAX_RULES
        && rules.iter().map(|rule| rule.groups.len()).sum::<usize>() <= MAX_RULES
        && rules.iter().all(|rule| {
            !rule.groups.is_empty()
                && rule.groups.len() <= MAX_RULES
                && rule.groups.iter().all(|group| {
                    !group.is_empty()
                        && group.len() <= 32
                        && group.iter().all(|name| {
                            !name.is_empty()
                                && name.len() <= 128
                                && name.contains(':')
                                && name
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"_:./-".contains(&b))
                        })
                })
                && (1..=127).contains(&rule.border)
                && rule.clauses.len() <= MAX_CLAUSES
                && rule.clauses.iter().all(|clause| {
                    clause.tiles & !0x1ff == 0
                        && clause.conditions.len() <= MAX_CONDITIONS
                        && clause.conditions.iter().all(|condition| {
                            condition.offset != [0; 3]
                                && condition.offset.iter().all(|v| (-1..=1).contains(v))
                        })
                })
        })
}
