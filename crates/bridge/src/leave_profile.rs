//! One diagnostic timeline from an accepted leave action through menu rendering and retirement.
//!
//! Milestones are cumulative milliseconds from the accepted UI action. Section durations include
//! worker queue waits and can overlap. Render preparation includes CPU resource retirement; the
//! GPU fence confirms preceding submissions have completed. Hidden runs name GPU frame completion
//! separately from window presentation. Reporting waits for the captured retirement sections.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use web_time::Instant;

static NEXT: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<LeaveProfile>> = Mutex::new(None);
thread_local! { static ACTIVE: std::cell::RefCell<LeaveProfile> = const { std::cell::RefCell::new(LeaveProfile(None)) }; }

/// Captures one leave independently of a later connection or leave.
#[derive(Clone, Default)]
pub struct LeaveProfile(Option<Arc<Attempt>>);

struct Attempt {
    id: u64,
    started: Instant,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sections: BTreeMap<&'static str, f64>,
    milestones: BTreeMap<&'static str, f64>,
    pending: usize,
    menu_ready: bool,
    menu_completed: bool,
    reported: bool,
}

/// Keeps one asynchronous retirement section open until its owner finishes.
pub struct Section {
    profile: LeaveProfile,
    name: &'static str,
    started: Instant,
}

/// Restores the previous thread's diagnostic context when a synchronous leave ends.
pub struct Binding {
    previous: LeaveProfile,
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl LeaveProfile {
    /// Starts at the accepted UI action, before the session controller handles it.
    pub fn begin() -> Self {
        let profile = Self(Some(Arc::new(Attempt {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            started: Instant::now(),
            state: Mutex::new(State::default()),
        })));
        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(profile.clone());
        profile.mark("click");
        profile
    }

    /// Captures the most recent action whose menu has not yet completed rendering.
    pub fn pending_presentation() -> Self {
        PENDING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_default()
    }

    /// Captures only this thread's leave, so unrelated joins cannot add stages to it.
    pub fn current() -> Self {
        ACTIVE.with(|slot| slot.borrow().clone())
    }

    /// Binds synchronous cleanup to this leave until the returned guard is dropped.
    pub fn enter(&self) -> Binding {
        Binding {
            previous: ACTIVE.with(|slot| slot.replace(self.clone())),
            thread: std::marker::PhantomData,
        }
    }

    /// Records a cumulative milestone once, in milliseconds since the action.
    pub fn mark(&self, name: &'static str) {
        if let Some(attempt) = &self.0 {
            attempt
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .milestones
                .entry(name)
                .or_insert_with(|| attempt.started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    /// Times a section, including queue waits when its guard moves to a worker.
    pub fn section(&self, name: &'static str) -> Section {
        if let Some(attempt) = &self.0 {
            let mut state = attempt.state.lock().unwrap_or_else(|e| e.into_inner());
            state.pending += 1;
            state.sections.entry(name).or_default();
        }
        Section {
            profile: self.clone(),
            name,
            started: Instant::now(),
        }
    }

    /// Times only the first frame that reaches this section, even while GPU feedback is pending.
    pub fn section_once(&self, name: &'static str) -> Option<Section> {
        let attempt = self.0.as_ref()?;
        let mut state = attempt.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.reported || state.sections.contains_key(name) {
            return None;
        }
        state.sections.insert(name, 0.0);
        state.pending += 1;
        Some(Section {
            profile: self.clone(),
            name,
            started: Instant::now(),
        })
    }

    /// Arms presentation only after the controller has exposed the menu's state.
    pub fn menu_ready(&self) {
        self.mark("menu_ready");
        if let Some(attempt) = &self.0 {
            attempt
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .menu_ready = true;
        }
    }

    /// Whether this extracted frame can be the first menu frame of this leave.
    pub fn presentation_pending(&self) -> bool {
        self.0.as_ref().is_some_and(|attempt| {
            let state = attempt.state.lock().unwrap_or_else(|e| e.into_inner());
            state.menu_ready && !state.menu_completed
        })
    }

    /// Records GPU completion of an extracted menu frame; hidden runs are identified explicitly.
    pub fn menu_completed(&self, hidden: bool) {
        self.mark(if hidden {
            "first_menu_frame_completed"
        } else {
            "first_menu_presented"
        });
        if let Some(attempt) = &self.0 {
            let mut state = attempt.state.lock().unwrap_or_else(|e| e.into_inner());
            state.menu_completed = true;
            attempt.report(&mut state);
        }
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if pending
            .as_ref()
            .and_then(|p| p.0.as_ref())
            .zip(self.0.as_ref())
            .is_some_and(|(a, b)| Arc::ptr_eq(a, b))
        {
            *pending = None;
        }
    }
}

impl Attempt {
    /// Emits one record only after the menu and all registered retirement sections finish.
    fn report(&self, state: &mut State) {
        if state.reported || !state.menu_completed || state.pending != 0 {
            return;
        }
        state.reported = true;
        let fields = state
            .milestones
            .iter()
            .map(|(name, ms)| format!("{name}_ms={ms:.3}"))
            .chain(
                state
                    .sections
                    .iter()
                    .map(|(name, ms)| format!("{name}_duration_ms={ms:.3}")),
            )
            .collect::<Vec<_>>()
            .join(" ");
        tracing::info!(target: "session", "CINNABAR_LEAVE id={} {}", self.id, fields);
    }
}

impl Drop for Section {
    /// Finishes the captured section without consulting the current session.
    fn drop(&mut self) {
        if let Some(attempt) = &self.profile.0 {
            let mut state = attempt.state.lock().unwrap_or_else(|e| e.into_inner());
            *state.sections.entry(self.name).or_default() +=
                self.started.elapsed().as_secs_f64() * 1000.0;
            state.pending -= 1;
            attempt.report(&mut state);
        }
    }
}

impl Drop for Binding {
    /// Restores the enclosing synchronous diagnostic context.
    fn drop(&mut self) {
        ACTIVE.with(|slot| slot.replace(std::mem::take(&mut self.previous)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates an isolated timeline without replacing another test's pending presentation.
    fn profile() -> LeaveProfile {
        LeaveProfile(Some(Arc::new(Attempt {
            id: 0,
            started: Instant::now(),
            state: Mutex::new(State::default()),
        })))
    }

    /// Reporting waits for both the first menu frame and the independently captured cleanup.
    #[test]
    fn completion_waits_for_menu_and_retirement_in_either_order() {
        for menu_first in [false, true] {
            let leave = profile();
            let section = leave.section("world_release");
            leave.menu_ready();
            if menu_first {
                leave.menu_completed(true);
            }
            assert!(!leave.0.as_ref().unwrap().state.lock().unwrap().reported);
            drop(section);
            if !menu_first {
                assert!(!leave.0.as_ref().unwrap().state.lock().unwrap().reported);
                leave.menu_completed(true);
            }
            assert!(leave.0.as_ref().unwrap().state.lock().unwrap().reported);
        }
    }

    /// Repeated frames waiting for GPU feedback contribute only their first preparation section.
    #[test]
    fn first_frame_sections_are_admitted_once() {
        let leave = profile();
        let first = leave.section_once("render_prepare_and_release").unwrap();
        assert!(leave.section_once("render_prepare_and_release").is_none());
        drop(first);
        assert!(leave.section_once("render_prepare_and_release").is_none());
        leave.menu_ready();
        leave.menu_completed(true);
        assert!(leave.section_once("late").is_none());
        assert!(leave.0.as_ref().unwrap().state.lock().unwrap().reported);
    }

    /// An old worker's completion cannot close a later leave or leak into a join's context.
    #[test]
    fn captured_retirement_is_independent_from_later_sessions() {
        let old = profile();
        let next = profile();
        let worker = old.section("network_shutdown");
        {
            let _old = old.enter();
            assert!(Arc::ptr_eq(
                LeaveProfile::current().0.as_ref().unwrap(),
                old.0.as_ref().unwrap()
            ));
            {
                let _next = next.enter();
                assert!(Arc::ptr_eq(
                    LeaveProfile::current().0.as_ref().unwrap(),
                    next.0.as_ref().unwrap()
                ));
            }
            assert!(Arc::ptr_eq(
                LeaveProfile::current().0.as_ref().unwrap(),
                old.0.as_ref().unwrap()
            ));
        }
        assert!(LeaveProfile::current().0.is_none());
        old.menu_completed(true);
        drop(worker);
        assert!(old.0.as_ref().unwrap().state.lock().unwrap().reported);
        assert!(!next.0.as_ref().unwrap().state.lock().unwrap().reported);
    }
}
