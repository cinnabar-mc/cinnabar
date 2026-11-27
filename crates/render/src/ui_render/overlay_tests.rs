use super::*;

#[test]
fn retained_hand_split_keeps_mixed_bucket_layer_blend_and_shadow_fill_order() {
    let plan =
        render_model::UiTexturePlan::new(&[[1024, 1024], [2048, 2048], [256, 256], [2048, 2048]])
            .unwrap();
    let batches = [1, 0, 3, 2, 1]
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            UiRenderBatch::new(
                page,
                render_model::UiScissor::new(index as u32, 0, 20, 20),
                index as u32 * 18,
                18,
                if index == 2 {
                    UI_BLEND_INVERT
                } else {
                    UI_BLEND_ALPHA
                },
            )
        })
        .collect::<Vec<_>>();
    let trace = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
        .unwrap()
        .flat_map(|(index, batch, location)| {
            retained_batch_ranges(batch, Some(&(60..66)))
                .into_iter()
                .flatten()
                .map(move |range| {
                    (
                        index,
                        location.bucket,
                        location.layer,
                        batch.blend_mode,
                        batch.scissor.x,
                        range,
                    )
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        trace,
        vec![
            (0, 1, 0, UI_BLEND_ALPHA, 0, 0..18),
            (1, 0, 0, UI_BLEND_ALPHA, 1, 18..36),
            (2, 1, 1, UI_BLEND_INVERT, 2, 36..54),
            (3, 2, 0, UI_BLEND_ALPHA, 3, 54..60),
            (3, 2, 0, UI_BLEND_ALPHA, 3, 66..72),
            (4, 1, 0, UI_BLEND_ALPHA, 4, 72..90),
        ]
    );
    let full = resolved_batches(Some(7), &batches, plan.locations(), plan.buckets())
        .unwrap()
        .flat_map(|(_, batch, _)| retained_batch_ranges(batch, None).into_iter().flatten())
        .collect::<Vec<_>>();
    assert_eq!(full, vec![0..18, 18..36, 36..54, 54..72, 72..90]);
    assert!(resolved_batches(None, &batches, plan.locations(), plan.buckets()).is_none());
    let mut locations = plan.locations().to_vec();
    locations[1].layer = 2;
    assert!(
        resolved_batches(Some(7), &batches, &locations, plan.buckets()).is_none(),
        "late invalid physical layer must emit no prefix"
    );
}
