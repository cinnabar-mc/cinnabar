//! View paths and named-source relationships retain storage until the expanded tree changes.

use super::*;

#[test]
fn refreshing_one_caption_reuses_view_path_storage() {
    let mut children = vec![control(
        "caption",
        "label",
        json!({"text":"#text", "bindings":[{
            "binding_name":"#caption", "binding_name_override":"#text"
        }]}),
        vec![],
    )];
    children.extend((0..512).map(|index| {
        control(
            &format!("fixed{index}"),
            "label",
            json!({"text":"#text", "property_bag":{"#fixed":"200"}, "bindings":[{
                "binding_type":"view", "source_property_name":"#fixed",
                "target_property_name":"#text"
            }]}),
            vec![],
        )
    }));
    let root = Arc::new(control("root", "panel", json!({}), children));
    let mut data = DataSource::new();
    data.set_global("#caption", Scalar::Text("first".into()));
    let mut state = BindState::new();
    let data = Arc::new(data);
    let mut bound = crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state);
    let mut measures = crate::MeasureCache::default();
    crate::rebind(
        &root,
        &data,
        &EmptyLibrary,
        &mut state,
        &mut bound,
        &mut measures,
    );
    let mut next = (*data).clone();
    next.set_global("#caption", Scalar::Text("second".into()));
    let next = Arc::new(next);
    let (_, allocations) = test_alloc::count(|| {
        crate::rebind(
            &root,
            &next,
            &EmptyLibrary,
            &mut state,
            &mut bound,
            &mut measures,
        );
    });
    let cold = crate::bind_incremental(&root, &next, &EmptyLibrary, &mut BindState::new());
    assert_eq!(captions(&bound), captions(&cold));
    assert!(
        allocations < 128,
        "view refresh allocated {allocations} times"
    );
}

#[test]
fn refreshing_one_caption_reuses_named_view_sources() {
    let mut children = vec![reader("caption", "#caption")];
    children.extend((0..512).map(|index| {
        control(
            &format!("pair{index}"),
            "panel",
            json!({}),
            vec![
                control("source", "panel", json!({"property_bag":{"#caption":"kept"}}), vec![]),
                control("reader", "label", json!({"text":"#text", "bindings":[{
                    "binding_type":"view", "source_control_name":"source", "resolve_sibling_scope":true,
                    "source_property_name":"#caption", "target_property_name":"#text"
                }]}), vec![]),
            ],
        )
    }));
    let root = Arc::new(control("root", "panel", json!({}), children));
    let mut data = DataSource::new();
    data.set_global("#caption", Scalar::Text("first".into()));
    let mut data = Arc::new(data);
    let mut state = BindState::new();
    let mut bound = crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state);
    let mut measures = crate::MeasureCache::default();
    crate::rebind(
        &root,
        &data,
        &EmptyLibrary,
        &mut state,
        &mut bound,
        &mut measures,
    );
    Arc::make_mut(&mut data).set_global("#caption", Scalar::Text("next".into()));
    let (_, allocations) = test_alloc::count(|| {
        crate::rebind(
            &root,
            &data,
            &EmptyLibrary,
            &mut state,
            &mut bound,
            &mut measures,
        );
    });
    let captions = captions(&bound);
    assert_eq!(captions[0], "next");
    assert!(captions[1..].iter().all(|caption| *caption == "kept"));
    assert!(
        allocations < 128,
        "named view refresh allocated {allocations} times"
    );
}

#[test]
fn named_sources_follow_controls_revealed_and_hidden_before_the_previous_winner() {
    let source = |caption| {
        control(
            "source",
            "panel",
            json!({"property_bag":{"#caption":caption}}),
            vec![],
        )
    };
    let root = Arc::new(control(
        "root",
        "panel",
        json!({}),
        vec![
            control(
                "reader",
                "label",
                json!({"text":"#text", "bindings":[{
                    "binding_type":"view", "source_control_name":"source",
                    "source_property_name":"#caption", "target_property_name":"#text"
                }]}),
                vec![],
            ),
            control(
                "first",
                "panel",
                json!({"visible":false, "bindings":[{
                    "binding_name":"#show_first", "binding_name_override":"#visible"
                }]}),
                vec![source("first")],
            ),
            control("second", "panel", json!({}), vec![source("second")]),
        ],
    ));
    let mut data = Arc::new(DataSource::new());
    Arc::make_mut(&mut data).set_global("#show_first", Scalar::Bool(false));
    let mut state = BindState::new();
    let mut bound = crate::bind_incremental(&root, &data, &EmptyLibrary, &mut state);
    assert_eq!(captions(&bound), ["second"]);
    let mut measures = crate::MeasureCache::default();
    for (shown, caption) in [(true, "first"), (false, "second"), (true, "first")] {
        Arc::make_mut(&mut data).set_global("#show_first", Scalar::Bool(shown));
        crate::rebind(
            &root,
            &data,
            &EmptyLibrary,
            &mut state,
            &mut bound,
            &mut measures,
        );
        assert_eq!(captions(&bound), [caption]);
    }
}
