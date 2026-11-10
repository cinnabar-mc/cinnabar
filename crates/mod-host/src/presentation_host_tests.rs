use super::*;
use crate::ModGrants;
use wit::Host as _;

/// A host state with the given grants and no package declarations.
fn state(grants: ModGrants) -> State {
    State::new(grants, String::new(), Default::default())
}

/// HUD authority with the given actor grants.
fn hud(players: bool, entities: bool) -> State {
    state(ModGrants {
        hud: true,
        players,
        entities,
        ..Default::default()
    })
}

const WHITE: Rgba = Rgba {
    r: 1.,
    g: 1.,
    b: 1.,
    a: 1.,
};

/// One annotation with a nametag span after the name and a roster span before it.
fn annotation(runtime_id: u64, text: &str) -> wit::Annotation {
    wit::Annotation {
        runtime_id,
        nametag: vec![wit::TextSpan {
            text: text.into(),
            color: WHITE,
        }],
        nametag_position: wit::AnnotationPosition::After,
        roster: vec![wit::TextSpan {
            text: text.into(),
            color: WHITE,
        }],
        roster_position: wit::AnnotationPosition::Before,
    }
}

/// Every import requires hud authority; per-actor rows also require players or entities.
#[test]
fn presentation_requires_hud_and_actor_grants() {
    let mut denied = state(ModGrants {
        players: true,
        entities: true,
        ..Default::default()
    });
    assert!(denied.set_nametag_style(None).unwrap().is_err());
    assert!(denied.set_hurt_overlay(Some(WHITE), None).unwrap().is_err());
    assert!(denied.set_tooltip_lines(Vec::new()).unwrap().is_err());
    assert!(denied.set_chat_actions(Vec::new()).unwrap().is_err());
    assert!(
        denied
            .set_annotations(vec![annotation(5, "x")])
            .unwrap()
            .is_err()
    );
    let mut hud_only = hud(false, false);
    assert!(
        hud_only
            .set_annotations(vec![annotation(5, "x")])
            .unwrap()
            .is_err()
    );
    assert!(
        hud_only
            .set_actor_tints(vec![wit::ActorTint {
                runtime_id: 5,
                color: WHITE,
            }])
            .unwrap()
            .is_err()
    );
    assert!(hud_only.set_annotations(Vec::new()).unwrap().is_ok());
    assert!(hud_only.set_actor_tints(Vec::new()).unwrap().is_ok());
    for (players, entities) in [(true, false), (false, true)] {
        let mut granted = hud(players, entities);
        assert!(
            granted
                .set_annotations(vec![annotation(5, "x")])
                .unwrap()
                .is_ok()
        );
    }
}

/// Output stays staged until the callback commits, and only changes advance the revision.
#[test]
fn presentation_commits_transactionally_with_fresh_revisions() {
    let mut state = hud(true, false);
    state.presentation.begin();
    state.set_hurt_overlay(Some(WHITE), None).unwrap().unwrap();
    state
        .set_annotations(vec![annotation(9, "3")])
        .unwrap()
        .unwrap();
    assert!(state.presentation.output().0.is_empty());
    state.presentation.commit();
    let (output, revision) = state.presentation.output();
    assert_eq!(output.hurt_overlay.body, Some([1.; 4]));
    let nametag = output.annotations[&9].nametag.as_ref().unwrap();
    assert_eq!(&*nametag.spans[0].text, "3");
    assert_eq!(nametag.position, SpanPosition::After);
    assert_eq!(
        output.annotations[&9].roster.as_ref().unwrap().position,
        SpanPosition::Before
    );
    assert_ne!(revision, 0);
    state.presentation.begin();
    state.set_hurt_overlay(Some(WHITE), None).unwrap().unwrap();
    state.presentation.commit();
    assert_eq!(state.presentation.output().1, revision, "unchanged output");
    state.presentation.begin();
    state.set_annotations(Vec::new()).unwrap().unwrap();
    state.presentation.begin();
    state.presentation.commit();
    assert!(
        !state.presentation.output().0.annotations.is_empty(),
        "a new callback discards the previous callback's staged writes"
    );
}

/// Every WIT annotation position reaches the client unchanged on both surfaces.
#[test]
fn annotation_positions_map_one_to_one() {
    let mut state = hud(true, false);
    for (requested, position) in [
        (wit::AnnotationPosition::Before, SpanPosition::Before),
        (wit::AnnotationPosition::After, SpanPosition::After),
        (wit::AnnotationPosition::Above, SpanPosition::Above),
        (wit::AnnotationPosition::Below, SpanPosition::Below),
    ] {
        state.presentation.begin();
        let mut row = annotation(9, "3");
        row.nametag_position = requested;
        row.roster_position = requested;
        state.set_annotations(vec![row]).unwrap().unwrap();
        state.presentation.commit();
        let (output, _) = state.presentation.output();
        let annotation = &output.annotations[&9];
        assert_eq!(annotation.nametag.as_ref().unwrap().position, position);
        assert_eq!(annotation.roster.as_ref().unwrap().position, position);
    }
}

/// Malformed or oversized input is rejected without staging anything.
#[test]
fn presentation_rejects_unbounded_or_malformed_rows() {
    let mut state = hud(true, true);
    let invalid = [
        vec![annotation(0, "x")],
        vec![annotation(4, "x"), annotation(4, "y")],
        vec![annotation(4, "§c")],
        vec![annotation(4, &"x".repeat(64))],
        (1..=ui::mod_presentation::MAX_ANNOTATIONS as u64 + 1)
            .map(|id| annotation(id, "x"))
            .collect(),
    ];
    for rows in invalid {
        state.presentation.begin();
        assert!(state.set_annotations(rows).unwrap().is_err());
    }
    state.presentation.begin();
    let many = vec![
        wit::TextSpan {
            text: "x".into(),
            color: WHITE,
        };
        ui::mod_presentation::MAX_ANNOTATION_SPANS + 1
    ];
    let mut row = annotation(4, "x");
    row.nametag = many;
    assert!(state.set_annotations(vec![row]).unwrap().is_err());
    let bad = Rgba { r: 2., ..WHITE };
    assert!(state.set_hurt_overlay(None, Some(bad)).unwrap().is_err());
    assert!(
        state
            .set_actor_tints(vec![wit::ActorTint {
                runtime_id: 1,
                color: bad,
            }])
            .unwrap()
            .is_err()
    );
    state.presentation.begin();
    let tints = (1..=MAX_ACTOR_TINTS as u64 + 1)
        .map(|runtime_id| wit::ActorTint {
            runtime_id,
            color: WHITE,
        })
        .collect();
    assert!(state.set_actor_tints(tints).unwrap().is_err());
    let line = wit::TooltipLine {
        identifier: "minecraft:apple".into(),
        metadata: None,
        text: "Food".into(),
        color: WHITE,
    };
    assert!(
        state
            .set_tooltip_lines(vec![line.clone(); MAX_TOOLTIP_LINES + 1])
            .unwrap()
            .is_err()
    );
    state.presentation.begin();
    assert!(
        state
            .set_tooltip_lines(vec![wit::TooltipLine {
                text: "a\nb".into(),
                ..line
            }])
            .unwrap()
            .is_err()
    );
    let action = wit::ChatAction {
        text: "Help".into(),
        action: "help".into(),
    };
    assert!(
        state
            .set_chat_actions(vec![action.clone(); MAX_CHAT_ACTIONS + 1])
            .unwrap()
            .is_err()
    );
    assert!(
        state
            .set_chat_actions(vec![wit::ChatAction {
                action: "no spaces".into(),
                ..action
            }])
            .unwrap()
            .is_err()
    );
    state.presentation.begin();
    let style = wit::NametagStyle {
        hide: false,
        hide_players: false,
        hide_passengers: false,
        show_self: true,
        show_when_hud_hidden: false,
        opacity: 1.5,
        scale: 1.,
        shadow: false,
        border_width: 0.5,
        text_color: None,
        own_text_color: None,
        border_color: None,
        background_color: None,
    };
    assert!(state.set_nametag_style(Some(style)).unwrap().is_err());
    state.presentation.commit();
    assert!(state.presentation.output().0.is_empty());
}

/// Rows without spans on either surface are dropped, and every import shares one budget.
#[test]
fn empty_annotations_are_dropped_and_writes_are_bounded() {
    let mut state = hud(true, false);
    state.presentation.begin();
    let mut empty = annotation(3, "x");
    empty.nametag.clear();
    empty.roster.clear();
    state.set_annotations(vec![empty]).unwrap().unwrap();
    state.presentation.commit();
    assert!(state.presentation.output().0.annotations.is_empty());
    state.presentation.begin();
    for _ in 0..MAX_IMPORT_WRITES {
        state.set_chat_actions(Vec::new()).unwrap().unwrap();
    }
    assert!(state.set_chat_actions(Vec::new()).is_err(), "budget traps");
}
