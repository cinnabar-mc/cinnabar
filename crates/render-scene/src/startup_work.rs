//! Admits background world preparation after the menu's first completed GPU frame.

use bevy::prelude::{Res, Resource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Shares a menu launch's preparation boundary; direct joins keep eager GPU initialization.
#[derive(Clone, Default, Resource)]
pub struct WorldWorkAdmission(Option<Arc<AtomicBool>>);

impl WorldWorkAdmission {
    /// Creates a closed boundary for a launch that must present its menu first.
    pub fn after_menu() -> Self {
        Self(Some(Arc::new(AtomicBool::new(false))))
    }

    /// Starts background preparation after the menu's submitted GPU work completes.
    pub fn menu_presented(&self) {
        if let Some(presented) = &self.0 {
            presented.store(true, Ordering::Release);
        }
    }

    /// Reads the CPU preparation boundary across threads.
    pub fn can_prepare_assets(&self) -> bool {
        self.is_admitted()
    }

    /// Whether GPU initialization should run on workers instead of the direct-join startup path.
    pub fn prepares_in_background(&self) -> bool {
        self.0.is_some()
    }

    /// Reads background GPU admission across the main and render threads.
    pub fn is_admitted(&self) -> bool {
        self.0
            .as_ref()
            .is_none_or(|presented| presented.load(Ordering::Acquire))
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
        assert!(worker.prepares_in_background());
        assert!(!worker.can_prepare_assets());
        assert!(!worker.is_admitted());
        gate.menu_presented();
        assert!(worker.can_prepare_assets());
        assert!(worker.is_admitted());
        gate.menu_presented();
        assert!(worker.is_admitted());
        let direct = WorldWorkAdmission::default();
        assert!(direct.is_admitted());
        assert!(!direct.prepares_in_background());
    }
}
