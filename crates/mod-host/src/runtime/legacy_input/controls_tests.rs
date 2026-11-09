//! Exercises both canonical layouts through actual component imports and callbacks.
use crate::{ModGrants, ModHost, empty_controls};

/// Creates a guest with either frozen controls/selection or current admission records.
fn guest(current: bool, denied: bool) -> String {
    let full = if denied {
        String::new()
    } else {
        format!(
            "i32.const 132 f32.load f32.const 0.25 f32.ne if unreachable end
         i32.const 136 i32.load8_u i32.const 1 i32.ne if unreachable end
         i32.const 152 i32.load i32.const 2 i32.ne if unreachable end
         i32.const {} f32.load f32.const 0 f32.ne if unreachable end {}",
            if current { 184 } else { 176 },
            if current {
                "i32.const 160 i32.load i32.const 1 i32.ne if unreachable end"
            } else {
                ""
            }
        )
    };
    let selected = if denied {
        ""
    } else {
        "i32.const 280 i32.load i32.const 1 i32.ne if unreachable end
         i32.const 276 i32.load i32.load i32.load i32.const 0x73756f4d i32.ne if unreachable end"
    };
    include_str!("../../tests/controls_0_1.wat")
        .replace(
            "$ADMITTED",
            if current {
                "(field \"keys-admitted\" (list string))"
            } else {
                ""
            },
        )
        .replace(
            "$SELECT_ADMITTED",
            if current {
                "(field \"keys-admitted\" (option (list string)))"
            } else {
                ""
            },
        )
        .replace("$SELECT_PARAMS", if current { "i32 i32 i32" } else { "" })
        .replace(
            "$SELECT_ARGS",
            if current {
                "i32.const 1 i32.const 1024 i32.const 1"
            } else {
                ""
            },
        )
        .replace("$DENIED", if denied { "1" } else { "0" })
        .replace("$FULL_CHECK", &full)
        .replace(
            "$SELECT_CHECK",
            &format!(
                "{selected} {}",
                if current && !denied {
                    "i32.const 288 i32.load i32.const 1 i32.ne if unreachable end"
                } else {
                    ""
                }
            ),
        )
}

#[test]
fn old_and_current_controls_records_instantiate_and_run_with_the_same_grants() {
    for current in [false, true] {
        for denied in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut host = ModHost::load_snapshot_with_grants(
                &directory.path().join("controls.wat"),
                guest(current, denied).as_bytes(),
                ModGrants {
                    controls: !denied,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut controls = empty_controls();
            controls.seconds = 0.25;
            controls.focused = true;
            controls.keys_held = vec!["KeyK".into(), "MouseLeft".into()];
            controls.keys_admitted = vec!["MouseLeft".into()];
            controls.wheel = -2.;
            host.frame_with_controls(true, None, controls).unwrap();
            assert!(host.is_active());
            assert_eq!(
                host.reserved_keys(),
                if denied {
                    Vec::<String>::new()
                } else {
                    vec!["MouseLeft".into()]
                }
            );
        }
    }
}
