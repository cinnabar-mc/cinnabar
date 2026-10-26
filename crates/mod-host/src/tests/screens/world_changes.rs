use super::*;
use mod_api::drawing::{DrawingPermissions, DrawingSession};

#[test]
fn loading_gap_preserves_undelivered_catalog_notifications() {
    use crate::{CallbackBudget, CallbackOutcome};
    use server_experience::runtime::{CALLBACK_FUEL, LOAD_FUEL};
    let (_dir, mut host) = probe();
    let original = DrawingSession {
        connection: 1,
        permissions: DrawingPermissions::ALL,
        ..Default::default()
    };
    host.bind_drawing_session(original);
    host.reload_if_changed().unwrap();
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.set_frame_authority(budget.clone(), false, false, 0);
    assert!(budget.reserve(CALLBACK_FUEL).is_some());
    assert!(budget.reserve(LOAD_FUEL).is_some());
    let snapshot = session(7, 1);
    let changed = host.update_session(Arc::clone(&snapshot)).unwrap();
    assert_eq!(
        host.dispatch(vec![ModEvent::ScreenChanged(Some(layout())), changed])
            .unwrap(),
        CallbackOutcome::Deferred
    );
    assert_eq!(value(&host, "#items"), Some(&Value::Integer(0)));
    host.bind_drawing_world(original, false);
    host.bind_drawing_session(DrawingSession {
        dimension: 1,
        ..original
    });
    assert!(host.update_session(snapshot).is_none());
    budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
    host.frame(false).unwrap();
    assert!(host.is_active());
    assert_eq!(value(&host, "#items"), Some(&Value::Integer(7)));
    assert_eq!(value(&host, "#data_layout_open"), Some(&Value::Bool(false)));
}
