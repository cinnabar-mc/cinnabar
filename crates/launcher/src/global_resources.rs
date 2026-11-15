//! Pack-management commands and immutable launcher snapshots.
use resource_pack::{ActivePack, InstalledPack};

/// Commands the settings host can submit without owning pack storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SelectAvailable(usize),
    SelectActive(usize),
    ReadMore(bool, usize),
    Activate(usize),
    Deactivate(usize),
    MoveUp(usize),
    MoveDown(usize),
    Settings(usize),
    CloseSettings,
    Subpack(usize),
    ToggleAvailable,
    ToggleActive,
    Import,
    Apply,
}

/// Immutable view of the staged selection and worker status.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Generation of the indexed pack lists, independent of presentation updates.
    pub revision: u64,
    pub memory_tier: u32,
    pub icons: std::collections::BTreeMap<(String, u64), String>,
    pub available: Vec<InstalledPack>,
    pub active: Vec<InstalledPack>,
    pub selection: Vec<ActivePack>,
    /// Exact last runtime acknowledgement; absent until initial pack preparation completes.
    pub applied_selection: Option<Vec<ActivePack>>,
    pub selected: Option<(bool, usize)>,
    pub details_expanded: Option<(bool, usize)>,
    pub settings: Option<usize>,
    pub available_expanded: bool,
    pub active_expanded: bool,
    pub busy: bool,
    pub message: String,
    /// Whether an active pack is read from the development folder, so Apply can reload edits.
    pub development_active: bool,
}

impl Snapshot {
    /// Whether the Apply changes action is enabled: for a changed selection, or to reload active
    /// development packs whose folders may have changed.
    pub fn can_apply(&self) -> bool {
        !self.busy
            && (self.has_pending_changes()
                || (self.development_active && self.applied_selection.is_some()))
    }

    /// Reports edits relative to the last acknowledged runtime stack.
    pub fn has_pending_changes(&self) -> bool {
        self.applied_selection
            .as_ref()
            .is_some_and(|applied| *applied != self.selection)
    }
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            memory_tier: 0,
            icons: Default::default(),
            available: Vec::new(),
            active: Vec::new(),
            selection: Vec::new(),
            applied_selection: None,
            selected: None,
            details_expanded: None,
            settings: None,
            available_expanded: true,
            active_expanded: true,
            busy: false,
            message: String::new(),
            development_active: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_reloads_active_development_packs_once_startup_has_applied() {
        let mut snapshot = Snapshot {
            development_active: true,
            ..Snapshot::default()
        };
        assert!(
            !snapshot.can_apply(),
            "startup preparation is still running"
        );
        snapshot.applied_selection = Some(Vec::new());
        assert!(snapshot.can_apply());
        assert!(!snapshot.has_pending_changes());
        snapshot.busy = true;
        assert!(!snapshot.can_apply());
        snapshot.busy = false;
        snapshot.development_active = false;
        assert!(!snapshot.can_apply());
    }
}
