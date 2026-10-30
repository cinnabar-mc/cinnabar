//! Scene policies read inherited root flags without building descendant controls.

use super::*;
use crate::{
    engine::screen::ScreenRegistry,
    sources::{ImageWork, RemoteImage, RemoteSource},
};
use std::sync::{Arc, LazyLock};

static REGISTRY: LazyLock<ScreenRegistry> =
    LazyLock::new(|| ScreenRegistry::new(Context::desktop()));

struct NoImages;
impl RemoteSource for NoImages {
    /// These original policy fixtures never read network artwork.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

/// An original screen with a variable-dependent inherited policy and unused child controls.
fn engine(children: usize, kind: &str) -> FormEngine {
    let controls: Vec<_> = (0..children).map(|index| serde_json::json!({
        format!("child{index}"):{"type":"panel","controls":[{"label":{"type":"label","text":"unused"}}]}
    })).collect();
    let document = serde_json::to_vec(&serde_json::json!({
        "namespace":"policy",
        "base":{"type":kind,"$capture":true,"absorbs_input":"$capture","controls":controls},
        "screen@base":{"should_steal_mouse":"$mouse"}
    }))
    .unwrap();
    let catalog = json_ui::Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/policy.json"]}"#.as_slice(),
        ),
        ("ui/policy.json", document.as_slice()),
    ])
    .unwrap();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        catalog,
        0,
        &REGISTRY,
        Arc::new(NoImages),
        ImageWork::inline(),
    )
}

#[test]
fn scene_policy_work_is_independent_of_unused_descendants() {
    let small = engine(0, "screen");
    let large = engine(1024, "screen");
    let context = Context::desktop();
    let (expected, small_work) =
        test_alloc::measure(|| small.scene_settings("policy.screen", &context));
    let (actual, large_work) =
        test_alloc::measure(|| large.scene_settings("policy.screen", &context));
    assert_eq!(actual, expected);
    assert!(actual.absorbs_input);
    assert!(!actual.should_steal_mouse);
    assert_eq!(
        large_work, small_work,
        "scene policy traversed unused descendants"
    );
    let (_, repeated) = test_alloc::measure(|| large.scene_settings("policy.screen", &context));
    assert_eq!(repeated, test_alloc::Allocations::default());
}

#[test]
fn scene_policy_cache_follows_catalog_context_and_non_screen_roots() {
    for kind in ["screen", "panel"] {
        let mut engine = engine(4, kind);
        let context = Context::desktop();
        let expected = engine.scene_settings("policy.screen", &context);
        let changed = context.clone().with_flag("mouse", true);
        assert!(!expected.should_steal_mouse);
        assert!(
            engine
                .scene_settings("policy.screen", &changed)
                .should_steal_mouse
        );
        let replacement = engine.catalog().as_ref().clone();
        let mut replacement = replacement;
        replacement.apply_pack([(
            "ui/policy.json",
            br#"{"namespace":"policy","base":{"absorbs_input":false}}"#.as_slice(),
        )]);
        engine.catalog = Arc::new(replacement);
        assert!(
            !engine
                .scene_settings("policy.screen", &context)
                .absorbs_input
        );
        assert_eq!(
            engine.scene_settings("missing.screen", &context),
            json_ui::ScreenSettings::default()
        );
    }
}
