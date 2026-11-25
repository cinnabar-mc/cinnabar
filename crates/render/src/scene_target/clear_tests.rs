use crate::scene_target::clear::replaces_samples;
use bevy::render::render_resource::{
    BlendState, ColorTargetState, FragmentState, RenderPipelineDescriptor,
};
use bevy::render::render_resource::{CachedRenderPipelineId, ColorWrites, PipelineCache};

/// Queues a pipeline whose fragment stage has `targets`, or none.
fn queued(
    cache: &mut PipelineCache,
    targets: Option<Vec<Option<ColorTargetState>>>,
) -> CachedRenderPipelineId {
    cache.queue_render_pipeline(RenderPipelineDescriptor {
        fragment: targets.map(|targets| FragmentState {
            targets,
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[test]
fn only_unblended_pipelines_writing_every_channel_replace_samples() {
    let (mut app, _) = crate::queue_review_support::app();
    let mut cache = app.world_mut().remove_resource::<PipelineCache>().unwrap();
    let target = |blend, write_mask| {
        Some(vec![Some(ColorTargetState {
            format: render_scene::SCENE_COLOR_FORMAT,
            blend,
            write_mask,
        })])
    };
    let cases = [
        (target(None, ColorWrites::ALL), true),
        (target(None, ColorWrites::COLOR), false),
        (
            target(Some(BlendState::ALPHA_BLENDING), ColorWrites::ALL),
            false,
        ),
        (Some(vec![None]), false),
        (Some(Vec::new()), false),
        (None, false),
    ];
    let ids = cases.map(|(targets, expected)| (queued(&mut cache, targets), expected));
    bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    cache.process_queue();
    for (id, expected) in ids {
        assert_eq!(replaces_samples(&cache, id), expected);
    }
    assert!(!replaces_samples(&cache, CachedRenderPipelineId::INVALID));
}
