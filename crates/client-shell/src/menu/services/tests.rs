use super::*;
use std::time::Instant;

/// During play behind a hidden menu the service pass runs once per interval, the first frame at
/// once; anything a frame may wait on runs it every frame and restarts the pacing.
#[test]
fn hidden_play_paces_the_service_pass_and_anything_waited_on_runs_it() {
    let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
    menu.visible = false;
    let start = Instant::now();
    assert!(menu.services_due(true, start), "the first quiet frame runs");
    assert!(
        !menu.services_due(true, start + SERVICE_POLL_INTERVAL / 2),
        "later quiet frames wait for the interval"
    );
    assert!(menu.services_due(true, start + SERVICE_POLL_INTERVAL));
    let later = start + SERVICE_POLL_INTERVAL;
    assert!(
        menu.services_due(false, later),
        "outside play every frame runs"
    );
    menu.visible = true;
    assert!(
        menu.services_due(true, later),
        "a visible menu runs every frame"
    );
    assert!(menu.services_due(true, later));
    menu.visible = false;
    menu.feeds.profile_refresh_requested = true;
    assert!(
        menu.services_due(true, later),
        "a held core decision runs every frame"
    );
    menu.feeds.profile_refresh_requested = false;
    menu.skin_update_pending = true;
    assert!(
        menu.services_due(true, later),
        "a skin change on its way runs every frame"
    );
    menu.skin_update_pending = false;
    menu.intents.disconnect = true;
    assert!(menu.services_due(true, later));
    menu.intents.disconnect = false;
    assert!(
        menu.services_due(true, later),
        "the first quiet frame after waiting runs at once"
    );
    assert!(!menu.services_due(true, later));
}
