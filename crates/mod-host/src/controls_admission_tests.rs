//! Admission observation boundaries share the existing controls authority and allowance.
use super::*;
use crate::runtime::cinnabar::extension::input::{Host as _, Selection};

#[test]
fn admitted_selection_and_samples_reject_invalid_names_and_overflow() {
    let mut state = State::new(
        crate::ModGrants {
            controls: true,
            ..Default::default()
        },
        "{}".into(),
        Default::default(),
    );
    for keys in [
        vec![String::new()],
        vec!["bad-key".into()],
        vec!["x".repeat(mod_api::MAX_CONTROL_KEY_BYTES + 1)],
        vec!["MouseLeft".into(); mod_api::MAX_CONTROL_KEYS + 1],
    ] {
        state.controls.begin_frame();
        assert!(
            state
                .read_selected_controls(Selection {
                    keys_pressed: Some(Vec::new()),
                    keys_held: Some(Vec::new()),
                    keys_admitted: Some(keys.clone()),
                    events: false,
                })
                .unwrap()
                .is_err()
        );
        let mut frame = empty_controls();
        frame.keys_admitted = keys;
        assert!(validate_frame(&frame).is_err());
    }
    let mut frame = empty_controls();
    frame.keys_admitted =
        vec!["x".repeat(mod_api::MAX_CONTROL_KEY_BYTES); mod_api::MAX_CONTROL_KEYS];
    validate_frame(&frame).unwrap();
    state.controls.begin_frame();
    state.controls.frame = frame.clone();
    for _ in 0..super::MAX_IMPORT_WRITES {
        assert_eq!(
            state
                .read_selected_controls(Selection {
                    keys_pressed: Some(Vec::new()),
                    keys_held: Some(Vec::new()),
                    keys_admitted: None,
                    events: false,
                })
                .unwrap()
                .unwrap()
                .keys_admitted,
            frame.keys_admitted
        );
    }
    assert!(state.read_controls().is_err());
    state.controls.begin_frame();
    state.grants.controls = false;
    assert!(
        state
            .read_selected_controls(Selection {
                keys_pressed: None,
                keys_held: None,
                keys_admitted: None,
                events: true,
            })
            .unwrap()
            .is_err()
    );
}
