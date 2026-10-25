//! Admits background world preparation after the menu's first completed GPU frame.

use bevy::prelude::{Res, Resource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Shares the first-menu boundary with CPU and GPU preparation workers.
#[derive(Clone, Resource)]
pub struct WorldWorkAdmission(Arc<AtomicBool>);

impl Default for WorldWorkAdmission {
    /// Keeps direct joins, tests and isolated renderers independent of a menu.
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}

impl WorldWorkAdmission {
    /// Creates a closed boundary for a launch that must present its menu first.
    pub fn after_menu() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Starts background preparation after the menu's submitted GPU work completes.
    pub fn menu_presented(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Reads the CPU preparation boundary across threads.
    pub fn can_prepare_assets(&self) -> bool {
        self.is_admitted()
    }

    /// Reads background GPU admission across the main and render threads.
    pub fn is_admitted(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Gates world preparation while allowing renderers without a startup menu to run normally.
pub fn world_work_allowed(admission: Option<Res<WorldWorkAdmission>>) -> bool {
    admission.is_none_or(|admission| admission.is_admitted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_menu_admits_world_work_without_a_connection_request() {
        let gate = WorldWorkAdmission::after_menu();
        let worker = gate.clone();
        assert!(!worker.can_prepare_assets());
        assert!(!worker.is_admitted());
        gate.menu_presented();
        assert!(worker.can_prepare_assets());
        assert!(worker.is_admitted());
        gate.menu_presented();
        assert!(worker.is_admitted());
        assert!(WorldWorkAdmission::default().is_admitted());
    }
}
