//! Separates menu completion from demand for world GPU work.

use bevy::prelude::{Res, Resource};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const MENU_PRESENTED: u8 = 1;
const WORLD_REQUESTED: u8 = 2;
const ADMITTED: u8 = MENU_PRESENTED | WORLD_REQUESTED;

/// CPU assets may prepare after the menu; world GPU work also needs a connection request.
#[derive(Clone, Resource)]
pub struct WorldWorkAdmission(Arc<AtomicU8>);

impl Default for WorldWorkAdmission {
    /// Keeps direct joins, tests and isolated renderers independent of a menu.
    fn default() -> Self {
        Self(Arc::new(AtomicU8::new(ADMITTED)))
    }
}

impl WorldWorkAdmission {
    /// Creates a closed boundary for a launch that must present its menu first.
    pub fn after_menu() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }

    /// Allows CPU preparation once the menu's submitted GPU work completes.
    pub fn menu_presented(&self) {
        self.0.fetch_or(MENU_PRESENTED, Ordering::AcqRel);
    }

    /// Records world demand without allowing it to delay the first menu image.
    pub fn request_world(&self) {
        self.0.fetch_or(WORLD_REQUESTED, Ordering::AcqRel);
    }

    /// Reads the CPU preparation boundary independently of world GPU demand.
    pub fn can_prepare_assets(&self) -> bool {
        self.0.load(Ordering::Acquire) & MENU_PRESENTED != 0
    }

    /// Reads admission across the main and render threads.
    pub fn is_admitted(&self) -> bool {
        self.0.load(Ordering::Acquire) == ADMITTED
    }
}

/// Gates world-only systems while allowing renderers without a startup menu to run normally.
pub fn world_work_allowed(admission: Option<Res<WorldWorkAdmission>>) -> bool {
    admission.is_none_or(|admission| admission.is_admitted())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_gpu_work_requires_both_menu_completion_and_world_demand() {
        for request_first in [false, true] {
            let gate = WorldWorkAdmission::after_menu();
            let worker = gate.clone();
            assert!(!worker.can_prepare_assets());
            assert!(!worker.is_admitted());
            if request_first {
                gate.request_world();
                assert!(!worker.can_prepare_assets());
                assert!(!worker.is_admitted());
            }
            gate.menu_presented();
            assert!(worker.can_prepare_assets());
            assert_eq!(worker.is_admitted(), request_first);
            gate.request_world();
            gate.menu_presented();
            assert!(worker.is_admitted());
        }
        assert!(WorldWorkAdmission::default().is_admitted());
    }
}
