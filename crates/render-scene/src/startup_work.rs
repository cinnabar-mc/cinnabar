//! Admits a menu launch's background world preparation in stages, by the platform's policy.

use bevy::prelude::{Res, Resource};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// When a menu launch starts the world's shader work: GPU owners, engine pipeline initializers
/// and pipeline warm-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldGpuStart {
    /// Behind the idle menu, once its first frame has completed.
    AfterMenu,
    /// Once the player starts a join. Until then the menu and Settings compile only what they
    /// draw, so a driver compiler fault or a long compile in a world shader cannot take them down.
    OnJoin,
}

impl WorldGpuStart {
    /// The startup policy of the platform this build targets.
    ///
    /// Phones wait for a join: their drivers' shader compilers have crashed and stalled on the
    /// world set, their compile pools hold one or two workers, and the system ends an app whose
    /// frame thread stays blocked through a lifecycle change. Android and iOS share the launch
    /// path and the compile queue, so they share the policy.
    pub const PLATFORM: Self = if cfg!(any(target_os = "android", target_os = "ios")) {
        Self::OnJoin
    } else {
        Self::AfterMenu
    };
}

/// One menu launch's shared progress through its preparation stages.
#[derive(Debug)]
struct Boundary {
    start: WorldGpuStart,
    menu_presented: AtomicBool,
    shaders_requested: AtomicBool,
    interface_pending: AtomicBool,
}

/// Shares a menu launch's preparation boundary; direct joins keep eager GPU initialization.
///
/// World data (CPU assets and texture uploads) is admitted once the menu's first frame has
/// completed. World shader work is admitted by [`WorldGpuStart`], and then only after the
/// interface's own pipelines have settled where a renderer tracks them.
#[derive(Clone, Default, Resource)]
pub struct WorldWorkAdmission(Option<Arc<Boundary>>);

impl WorldWorkAdmission {
    /// Creates a closed boundary for a launch that must present its menu first.
    pub fn after_menu(start: WorldGpuStart) -> Self {
        Self(Some(Arc::new(Boundary {
            start,
            menu_presented: AtomicBool::new(false),
            shaders_requested: AtomicBool::new(false),
            interface_pending: AtomicBool::new(false),
        })))
    }

    /// Records the menu's first completed GPU frame: world data starts preparing, and so does
    /// shader work unless the policy waits for a join.
    pub fn menu_presented(&self) {
        if let Some(boundary) = &self.0 {
            boundary.menu_presented.store(true, Ordering::Release);
            if boundary.start == WorldGpuStart::AfterMenu {
                boundary.shaders_requested.store(true, Ordering::Release);
            }
        }
    }

    /// Records that the player started a join, which requests shader work under every policy.
    pub fn join_requested(&self) {
        if let Some(boundary) = &self.0 {
            boundary.shaders_requested.store(true, Ordering::Release);
        }
    }

    /// Reads the world-data boundary across threads: CPU assets and texture uploads, which
    /// compile nothing.
    pub fn can_prepare_assets(&self) -> bool {
        self.0
            .as_ref()
            .is_none_or(|boundary| boundary.menu_presented.load(Ordering::Acquire))
    }

    /// Whether GPU initialization should run on workers instead of the direct-join startup path.
    pub fn prepares_in_background(&self) -> bool {
        self.0.is_some()
    }

    /// Whether shader work has been requested, which lets the interface warm its own pipelines
    /// ahead of the world's.
    pub fn shaders_requested(&self) -> bool {
        self.0
            .as_ref()
            .is_none_or(|boundary| boundary.shaders_requested.load(Ordering::Acquire))
    }

    /// Whether world shader work still waits for a join that has not been requested.
    pub fn awaits_join(&self) -> bool {
        self.0.as_ref().is_some_and(|boundary| {
            boundary.start == WorldGpuStart::OnJoin
                && !boundary.shaders_requested.load(Ordering::Acquire)
        })
    }

    /// Holds world shader work behind the interface's pipelines until
    /// [`Self::interface_settled`]. Only a renderer that tracks those pipelines calls this, so
    /// launches without one are never held. Direct joins stay eager.
    pub fn hold_for_interface(&self) {
        if let Some(boundary) = &self.0 {
            boundary.interface_pending.store(true, Ordering::Release);
        }
    }

    /// Releases the hold of [`Self::hold_for_interface`].
    pub fn interface_settled(&self) {
        if let Some(boundary) = &self.0 {
            boundary.interface_pending.store(false, Ordering::Release);
        }
    }

    /// Reads world shader admission across the main and render threads: requested, and not
    /// held behind the interface's pipelines.
    pub fn is_admitted(&self) -> bool {
        self.0.as_ref().is_none_or(|boundary| {
            boundary.shaders_requested.load(Ordering::Acquire)
                && !boundary.interface_pending.load(Ordering::Acquire)
        })
    }
}

/// Gates world shader work while allowing renderers without a startup menu to run normally.
pub fn world_work_allowed(admission: Option<Res<WorldWorkAdmission>>) -> bool {
    admission.is_none_or(|admission| admission.is_admitted())
}

/// Gates world texture uploads, which follow the menu's first frame on every platform because
/// the menu's own resource-pack changes wait for them.
pub fn world_uploads_allowed(admission: Option<Res<WorldWorkAdmission>>) -> bool {
    admission.is_none_or(|admission| admission.can_prepare_assets())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_menu_admits_world_work_without_a_connection_request() {
        let gate = WorldWorkAdmission::after_menu(WorldGpuStart::AfterMenu);
        let worker = gate.clone();
        assert!(worker.prepares_in_background());
        assert!(!worker.can_prepare_assets());
        assert!(!worker.is_admitted());
        gate.menu_presented();
        assert!(worker.can_prepare_assets());
        assert!(worker.is_admitted());
        assert!(!worker.awaits_join());
        gate.menu_presented();
        assert!(worker.is_admitted());
        let direct = WorldWorkAdmission::default();
        assert!(direct.is_admitted());
        assert!(direct.can_prepare_assets());
        assert!(direct.shaders_requested());
        assert!(!direct.awaits_join());
        assert!(!direct.prepares_in_background());
    }

    #[test]
    fn a_join_policy_keeps_shader_work_closed_behind_the_menu_until_a_join() {
        let gate = WorldWorkAdmission::after_menu(WorldGpuStart::OnJoin);
        let worker = gate.clone();
        assert!(worker.awaits_join());
        gate.menu_presented();
        gate.menu_presented();
        assert!(worker.can_prepare_assets(), "world data still prepares");
        assert!(!worker.shaders_requested());
        assert!(!worker.is_admitted());
        assert!(worker.awaits_join());
        gate.join_requested();
        assert!(worker.shaders_requested());
        assert!(worker.is_admitted());
        assert!(!worker.awaits_join());
    }

    #[test]
    fn a_join_before_the_menu_frame_requests_shader_work_under_either_policy() {
        for start in [WorldGpuStart::AfterMenu, WorldGpuStart::OnJoin] {
            let gate = WorldWorkAdmission::after_menu(start);
            gate.join_requested();
            assert!(gate.is_admitted());
            assert!(!gate.can_prepare_assets(), "data waits for the menu frame");
        }
    }

    #[test]
    fn an_interface_hold_delays_admission_but_not_the_request() {
        let gate = WorldWorkAdmission::after_menu(WorldGpuStart::AfterMenu);
        gate.hold_for_interface();
        gate.menu_presented();
        assert!(gate.shaders_requested());
        assert!(!gate.is_admitted());
        gate.interface_settled();
        assert!(gate.is_admitted());
        let direct = WorldWorkAdmission::default();
        direct.hold_for_interface();
        assert!(direct.is_admitted(), "direct joins are never held");
    }

    #[test]
    fn the_platform_policy_waits_for_a_join_only_on_phones() {
        let phone = cfg!(any(target_os = "android", target_os = "ios"));
        assert_eq!(WorldGpuStart::PLATFORM == WorldGpuStart::OnJoin, phone);
    }
}
