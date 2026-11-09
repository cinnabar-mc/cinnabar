use super::*;
use actor_animation::{ControllerActivity, RenderVisibility};

#[test]
fn visibility_removes_first_person_items_and_map_paper_independently_of_body() {
    for hide_body in [false, true] {
        for hide_items in [false, true] {
            let (stream, mut equipment, artwork, input) =
                native_tests::fixture(Some("minecraft:shield"));
            let mut source = native_tests::hand_source_for_mode(
                &stream,
                &mut equipment,
                &artwork,
                &input,
                &mut java::HandCache::default(),
                0.5,
                false,
            );
            assert!(source.items[0].is_some());
            // Original paper geometry stands in for a map's separate paper draw.
            source.maps.paper[0] = Some((source.presentation.submission.clone(), Mat4::IDENTITY));
            let had_body = source.body.is_some();
            let visibility = RenderVisibility {
                controllers: if hide_body {
                    ControllerActivity::Inactive
                } else {
                    ControllerActivity::Active
                },
                hide_held_items: Some(hide_items),
            };
            source.apply_visibility(visibility);
            assert_eq!(source.body.is_some(), had_body && !hide_body);
            assert_eq!(source.items[0].is_some(), !hide_items);
            assert_eq!(source.maps.paper[0].is_some(), !hide_items);
            if hide_body && hide_items {
                assert_eq!(source.all_submissions().count(), 0);
                assert!(
                    is_ready(
                        &ActorRigFrameBuilder::new([]).unwrap(),
                        Some(&source),
                        Some(1.0)
                    ),
                    "an intentionally empty hand must not request a fallback draw"
                );
            }
        }
    }
}
