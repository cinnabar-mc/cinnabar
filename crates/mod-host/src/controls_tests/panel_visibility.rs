use super::*;

#[test]
fn opening_panel_is_granted_transactional_and_requires_content() {
    use cinnabar::extension::panel::Host as _;
    let mut state = State::new(ModGrants::default(), String::new(), Default::default());
    assert!(state.open().unwrap().is_err());
    state.grants.controls = true;
    assert!(state.open().unwrap().is_err());
    state.set_content(panel()).unwrap().unwrap();
    assert!(state.open().unwrap().is_ok());
    assert!(!state.controls.open);
    state.controls.commit();
    assert!(state.controls.open);
    state.close().unwrap().unwrap();
    assert!(state.controls.open);
    state.controls.revoke();
    state.controls.commit();
    assert!(!state.controls.open);
}

#[test]
fn panel_visibility_calls_compose_within_a_callback_and_require_authority() {
    use cinnabar::extension::panel::Host as _;
    let mut state = State::new(ModGrants::default(), String::new(), Default::default());
    assert!(state.close().unwrap().is_err());
    assert!(state.toggle().unwrap().is_err());
    state.grants.controls = true;
    assert!(state.toggle().unwrap().is_err());
    state.close().unwrap().unwrap();
    state.controls.commit();
    assert!(!state.controls.open);
    state.controls.reset_budget();
    state.set_content(panel()).unwrap().unwrap();
    state.open().unwrap().unwrap();
    state.open().unwrap().unwrap();
    assert!(!state.controls.open);
    state.controls.commit();
    assert!(state.controls.open);
    state.controls.reset_budget();
    state.toggle().unwrap().unwrap();
    state.toggle().unwrap().unwrap();
    state.controls.commit();
    assert!(state.controls.open);
    state.controls.reset_budget();
    state.close().unwrap().unwrap();
    state.close().unwrap().unwrap();
    state.toggle().unwrap().unwrap();
    state.close().unwrap().unwrap();
    state.controls.commit();
    assert!(!state.controls.open);
    assert!(state.controls.panel.is_some());
    state.controls.reset_budget();
    state.toggle().unwrap().unwrap();
    state.controls.rollback();
    state.controls.commit();
    assert!(!state.controls.open);
}
