use super::super::*;
use crate::anim::AnimGraph;
use std::sync::Mutex;

/// A one-second alpha fade on the control at `key`.
fn fade(key: &str) -> Arc<ControlAnims> {
    let node = AnimNode::parse(
        serde_json::json!({"anim_type":"alpha","duration":1,"from":0,"to":1})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    Arc::new(ControlAnims {
        key: key.into(),
        graph: AnimGraph {
            heads: vec![0],
            nodes: vec![node],
        },
        rest_alpha: 1.0,
        rest_offset: [0.0; 2],
        rect: [0.0, 0.0, 10.0, 10.0],
        anchor: [0.0; 2],
        born: None,
        clock: None,
        disable_fast_forward: true,
        reset_name: None,
        has_sprite: false,
        layout_authoritative: true,
    })
}

/// An animator whose paint of `/settled` finished its fade, beside an unrecorded `/other`.
fn painted() -> (Animator, Arc<AnimationRecord>) {
    let mut animator = Animator::new();
    let settled = fade("/settled");
    animator.sample(&fade("/other"), 0.0, None);
    animator.sample(&settled, 0.0, None);
    animator.end_frame();
    animator.record();
    animator.sample(&settled, 2.0, None);
    let record = Arc::new(animator.finish_record());
    animator.sample(&fade("/other"), 2.0, None);
    animator.end_frame();
    (animator, record)
}

/// What later frames can observe: each component's sample time and whether it was touched.
fn observed(animator: &Animator) -> Vec<(String, f64, bool)> {
    let mut components: Vec<_> = animator
        .components
        .iter()
        .map(|(key, component)| (key.clone(), component.last, component.touched))
        .collect();
    components.sort_by(|a, b| a.0.cmp(&b.0));
    components
}

/// A deferred run of repeats, once applied, leaves what repeating each frame left: the
/// unrecorded component retired and the recorded one sampled at the last time.
#[test]
fn deferred_repeats_leave_what_immediate_repeats_leave() {
    let (mut immediate, record) = painted();
    for now in [3.0, 4.0, 5.0] {
        assert!(immediate.can_replay(&record));
        assert!(immediate.replay(&record, now));
        immediate.end_frame();
    }
    let (animator, record) = painted();
    let shared = super::SharedAnimator(Mutex::new(animator));
    for now in [3.0, 4.0, 5.0] {
        assert!(shared.defer_repeat(&[&record], now));
    }
    assert_eq!(observed(&shared.lock()), observed(&immediate));
    assert!(!shared.lock().components.contains_key("/other"));
}

/// A paint whose ticked component still plays is repeated by the caller, not deferred.
#[test]
fn a_playing_ticked_component_is_not_deferred() {
    let mut animator = Animator::new();
    let ticking = fade("/ticking");
    animator.record();
    animator.set_quiet(true);
    animator.sample(&ticking, 0.0, None);
    animator.set_quiet(false);
    let record = Arc::new(animator.finish_record());
    animator.end_frame();
    assert!(animator.can_replay(&record), "ticked components may play");
    assert!(!animator.defer_repeat(&[&record], 0.5));
}
