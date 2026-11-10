//! Armor trim identity shared by stack decoding and every item drawing route.
use std::sync::Arc;

/// Material and pattern identifiers stored on one trimmed stack.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemTrim {
    pub material: Arc<str>,
    pub pattern: Arc<str>,
}
