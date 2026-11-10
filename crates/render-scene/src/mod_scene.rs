use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use mod_api::MAX_BLOCK_HIGHLIGHTS;
use mod_render::{RenderOutput, block_highlights, geometry::ModVertex};
use std::sync::Arc;

/// Requests host-owned skin preparation for a selected actor even when its main body is hidden.
#[derive(Resource, Default)]
pub struct ActorCoverageRequests {
    pub connection: u64,
    pub dimension: i32,
    pub runtime_ids: Vec<u64>,
}

/// Publishes incarnation keys and the admitted body/equipment cost without model or pose data.
pub fn mask_targets(frame: &crate::ActorRenderFrame) -> Vec<(mod_api::drawing::MaskEntity, u64)> {
    let mut targets = std::collections::BTreeMap::<_, u64>::new();
    for (entry, instance) in frame.rig.manifest.iter().zip(frame.rig.instances.iter()) {
        let key = mod_api::drawing::MaskEntity {
            connection: entry.identity.session_id,
            dimension: entry.identity.dimension,
            runtime_id: entry.identity.runtime_id,
            spawn_revision: entry.identity.spawn_revision.get(),
        };
        let vertices = frame
            .rig
            .geometry_spans
            .get(instance.geometry_id as usize)
            .map_or(0, |span| u64::from(span.vertex_count));
        let total = targets.entry(key).or_default();
        *total = total.saturating_add(vertices);
    }
    targets.into_iter().collect()
}
/// The current mod's render output, extracted whenever the mod commits a change.
#[derive(Resource, Clone, Debug, Default)]
pub struct ModRenderScene {
    generation: u64,
    personal_generation: u64,
    server_generation: u64,
    personal: RenderOutput,
    server: RenderOutput,
    merge: mod_render::RenderMerge,
    pub passes: Vec<mod_render::Pass>,
    pub retained: Vec<Arc<mod_render::retained::Resources>>,
    pub gpu_reservations: Vec<mod_render::GpuReservation>,
    pub masks: Arc<[mod_render::masks::Mask]>,
    pub vertices: Arc<[ModVertex]>,
    pub bundle_vertices: Vec<(mod_render::GpuLedger, Arc<[ModVertex]>)>,
    primitives: Arc<mod_render::Primitives>,
    pub block_vertices: Arc<[ModVertex]>,
    drawn_blocks: Arc<[mod_api::native_render::BlockHighlight]>,
}

impl ExtractResource<bevy::render::RenderApp> for ModRenderScene {
    type Source = Self;

    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

impl ModRenderScene {
    /// Identifies a current server contributor so frame publication preserves its anchor policy.
    pub fn is_server_contributor(&self, resources: &Arc<mod_render::retained::Resources>) -> bool {
        self.server
            .retained
            .iter()
            .any(|server| Arc::ptr_eq(server, resources))
    }

    /// Adopts `output` unless `generation` is already applied.
    pub fn apply(&mut self, output: &RenderOutput, generation: u64) {
        if generation == self.personal_generation {
            return;
        }
        self.personal_generation = generation;
        self.personal = output.clone();
        self.publish_layers();
    }

    /// Publishes changed personal content without comparing it to the merged server layer.
    pub fn apply_if_changed(&mut self, output: &RenderOutput) {
        if self.personal != *output {
            self.apply(output, self.personal_generation.wrapping_add(1));
        }
    }

    /// Publishes session-owned client-part drawing beside personal mod output.
    pub fn apply_server(&mut self, output: &RenderOutput, generation: u64) -> bool {
        if generation == self.server_generation {
            return false;
        }
        self.server_generation = generation;
        self.server = output.clone();
        self.publish_layers();
        true
    }

    /// Combines both owners through the same renderer budgets and publication path.
    fn publish_layers(&mut self) {
        let output = self.merge.merge([&self.personal, &self.server]);
        self.generation = self.generation.wrapping_add(1);
        self.passes.clone_from(&output.passes);
        self.retained.clone_from(&output.retained);
        if !Arc::ptr_eq(&self.primitives, &output.primitives)
            || self
                .gpu_reservations
                .iter()
                .map(|r| (&r.ledger, &r.primitives))
                .ne(output
                    .gpu_reservations
                    .iter()
                    .map(|r| (&r.ledger, &r.primitives)))
        {
            self.primitives = Arc::clone(&output.primitives);
            self.vertices = mod_render::geometry::build_partition(
                &output.primitives,
                &output.gpu_reservations,
                None,
            )
            .into();
            self.bundle_vertices = output
                .gpu_reservations
                .iter()
                .enumerate()
                .map(|(index, r)| {
                    (
                        r.ledger.clone(),
                        mod_render::geometry::build_partition(
                            &output.primitives,
                            &output.gpu_reservations,
                            Some(index),
                        )
                        .into(),
                    )
                })
                .collect();
        }
        self.gpu_reservations.clone_from(&output.gpu_reservations);
        self.masks = Arc::clone(&output.masks);
    }

    /// Adopts a packet mod's validated block highlights, drawn through world geometry.
    pub fn set_drawn_block_highlights(
        &mut self,
        blocks: &[mod_api::native_render::BlockHighlight],
    ) {
        let blocks = &blocks[..blocks.len().min(MAX_BLOCK_HIGHLIGHTS)];
        if self.drawn_blocks.as_ref() == blocks {
            return;
        }
        self.drawn_blocks = Arc::from(blocks);
        self.block_vertices = block_highlights::build_styled(
            self.drawn_blocks
                .iter()
                .map(|block| (block.position, block.colour, block.outline.as_ref())),
        )
        .into();
    }

    /// Drops every pass and primitive, as when a mod traps, reloads or is revoked.
    pub fn clear(&mut self) {
        if self.generation != 0
            || !self.passes.is_empty()
            || !self.masks.is_empty()
            || self.vertex_count() != 0
        {
            *self = Self::default();
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
            + self.block_vertices.len()
            + self
                .bundle_vertices
                .iter()
                .map(|(_, vertices)| vertices.len())
                .sum::<usize>()
    }
}

#[cfg(test)]
mod block_highlights_tests;
#[cfg(test)]
mod layers_tests;

/// Same-frame placement and GPU inputs for retained world drawing.
pub mod retained;
