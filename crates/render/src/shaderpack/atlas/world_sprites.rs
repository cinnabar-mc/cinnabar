//! World material sprites for shader packs. A shader pack samples Bedrock blocks from the
//! Java-style atlas, so a block bound to a world material draws, in a pack, with an atlas sprite
//! of its own: each frame the material's guest shader renders its surface into the sprite's
//! square of the colour, normal and specular atlases, every mip level, and the pack lights it
//! as it lights any block sprite. Expansion redirects a bound face to the sprite once its
//! rectangle is published; until then, and after revocation, the face keeps its Bedrock sprite.

use super::pack::SpriteRect;
use super::upload::{LayerTextures, PBR_FORMAT, UploadedAtlas};
use crate::world_material::{ShaderFamily, WorldMaterialGpu, WorldVariant};
use bevy::{
    prelude::{Res, ResMut, Resource},
    render::{
        render_resource::{
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, MultisampleState,
            PipelineCache, PrimitiveState, RenderPipelineDescriptor, VertexState,
        },
        renderer::{PendingCommandBuffers, RenderDevice, RenderQueue},
    },
};
use render_scene::world_material::WorldMaterialScene;
use std::collections::{BTreeSet, HashMap};

/// The renderer namespace of world material sprite modules.
pub(crate) const SPRITE_SHADERS: u64 = u64::from_le_bytes(*b"wmsprit\0");
/// The sprite stage the guest library composes with.
const SPRITE_SOURCE: &str = include_str!("world_sprites.wgsl");
/// Remap words pack an x/y or width/height pair into one `u32`.
const REMAP_FIELD_BITS: u32 = 16;

/// How sprite modules compose: the guest library in group 0, then the sprite stage.
pub(crate) fn family() -> ShaderFamily {
    ShaderFamily {
        namespace: SPRITE_SHADERS,
        group: 0,
        build: |library, path| {
            crate::shader_safety::from_wgsl(format!("{library}\n{SPRITE_SOURCE}"), path)
        },
    }
}

/// The sprite pipeline of `variant`: its module, its bind group, and the three atlas targets.
fn descriptor(variant: WorldVariant) -> RenderPipelineDescriptor {
    let shader = variant.shader(SPRITE_SHADERS);
    let target = |format| {
        Some(ColorTargetState {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        })
    };
    RenderPipelineDescriptor {
        label: Some("world material sprite pipeline".into()),
        layout: vec![variant.layout()],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("world_sprite_vertex".into()),
            ..Default::default()
        },
        fragment: Some(FragmentState {
            shader,
            entry_point: Some("world_sprite_fragment".into()),
            targets: vec![
                target(super::upload::COLOR_STORAGE_FORMAT),
                target(PBR_FORMAT),
                target(PBR_FORMAT),
            ],
            ..Default::default()
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState::default(),
        ..Default::default()
    }
}

/// The published atlas's world material sprites: their squares, the per-mip render targets of
/// the three atlases, and which material each slot's remap entry currently shows.
#[derive(Resource)]
pub(crate) struct WorldMaterialSprites {
    rects: Vec<SpriteRect>,
    mip_levels: u32,
    /// Per mip level: the colour, normal and specular targets.
    targets: Vec<[wgpu::TextureView; 3]>,
    remap: wgpu::Buffer,
    /// The material key whose sprite each slot's remap entry shows.
    shown: Vec<Option<u64>>,
    /// Changes whenever a remap entry is published or cleared, which re-expands every section.
    pub(crate) revision: u64,
    _textures: LayerTextures,
}

impl WorldMaterialSprites {
    /// Tracks `uploaded`'s world sprite squares; none is shown until first rendered.
    pub(crate) fn new(uploaded: &UploadedAtlas, rects: Vec<SpriteRect>, mip_levels: u32) -> Self {
        let pbr = uploaded
            .atlas
            .pbr
            .as_ref()
            .expect("the atlas always uploads PBR layers");
        let view = |texture: &wgpu::Texture, format, level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("shader-pack world material sprite target"),
                format: Some(format),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let targets = (0..mip_levels)
            .map(|level| {
                [
                    view(
                        &uploaded.atlas.color,
                        super::upload::COLOR_STORAGE_FORMAT,
                        level,
                    ),
                    view(&pbr[0], PBR_FORMAT, level),
                    view(&pbr[1], PBR_FORMAT, level),
                ]
            })
            .collect();
        Self {
            shown: vec![None; rects.len()],
            rects,
            mip_levels,
            targets,
            remap: uploaded.remap.clone(),
            revision: 0,
            _textures: uploaded.atlas.clone(),
        }
    }

    /// Writes slot `slot`'s remap entry: its square when `shown`, else empty.
    fn publish(&self, queue: &wgpu::Queue, slot: usize, shown: bool) {
        let rect = self.rects[slot];
        let words: [u32; 2] = if shown {
            [
                rect.x | (rect.y << REMAP_FIELD_BITS),
                rect.side | (rect.side << REMAP_FIELD_BITS),
            ]
        } else {
            [0; 2]
        };
        let at = super::world_material_sprite_slot(slot as u32) as u64 * 8;
        queue.write_buffer(&self.remap, at, bytemuck::cast_slice(&words));
    }
}

/// Sprite pipelines of the published programs, by variant.
#[derive(Resource, Default)]
pub(crate) struct WorldSpritePipelines {
    ids: HashMap<WorldVariant, CachedRenderPipelineId>,
    /// The published revision `ids` was last pruned to.
    pruned: Option<u64>,
}

/// One world material sprite ready to render this frame.
pub(crate) struct ReadySprite<'a> {
    pub slot: usize,
    pub key: u64,
    pub pipeline: &'a wgpu::RenderPipeline,
    pub group: &'a wgpu::BindGroup,
}

impl WorldMaterialSprites {
    /// Records each `ready` sprite into every mip level of the three atlases, publishes the
    /// remap entry of each newly shown one and clears the entries of slots no longer ready.
    /// Returns whether anything was recorded.
    pub(crate) fn encode(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        ready: &[ReadySprite<'_>],
    ) -> bool {
        for slot in 0..self.rects.len() {
            if self.shown[slot].is_some() && !ready.iter().any(|sprite| sprite.slot == slot) {
                self.shown[slot] = None;
                self.publish(queue, slot, false);
                self.revision += 1;
            }
        }
        for sprite in ready.iter().filter(|sprite| sprite.slot < self.rects.len()) {
            for level in 0..self.mip_levels {
                let rect = self.rects[sprite.slot].at_mip(level);
                if rect.side == 0 {
                    break;
                }
                let attachments = self.targets[level as usize].each_ref().map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shader-pack world material sprite"),
                    color_attachments: &attachments,
                    ..Default::default()
                });
                let (x, y, side) = (rect.x as f32, rect.y as f32, rect.side as f32);
                pass.set_viewport(x, y, side, side, 0.0, 1.0);
                pass.set_scissor_rect(rect.x, rect.y, rect.side, rect.side);
                pass.set_pipeline(sprite.pipeline);
                pass.set_bind_group(0, sprite.group, &[]);
                pass.draw(0..3, 0..1);
            }
            if self.shown[sprite.slot] != Some(sprite.key) {
                self.shown[sprite.slot] = Some(sprite.key);
                self.publish(queue, sprite.slot, true);
                self.revision += 1;
            }
        }
        !ready.is_empty()
    }
}

/// Renders the sprite of every block-bound world material whose pipeline is ready and clears
/// those whose material is gone or not ready; the sprites join the frame's submission ahead
/// of the graph.
pub(crate) fn render_world_sprites(
    sprites: Option<ResMut<WorldMaterialSprites>>,
    world: Option<Res<WorldMaterialGpu>>,
    scene: Option<Res<WorldMaterialScene>>,
    (cache, mut pipelines): (Option<Res<PipelineCache>>, ResMut<WorldSpritePipelines>),
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
    mut pending: ResMut<PendingCommandBuffers>,
) {
    // Without a pipeline cache, as in an app that renders no world, no sprite can draw.
    let (Some(mut sprites), Some(cache)) = (sprites, cache) else {
        return;
    };
    let bound: BTreeSet<u16> = scene
        .as_deref()
        .map(|scene| scene.set.blocks.values().copied().collect())
        .unwrap_or_default();
    let slots = sprites.rects.len();
    let mut ready = Vec::new();
    if let Some(world) = world.as_deref() {
        let pipelines = &mut *pipelines;
        crate::world_material::prune_pipelines(
            &mut pipelines.ids,
            &mut pipelines.pruned,
            world,
            |variant| variant.program,
        );
        for slot in bound
            .iter()
            .map(|&slot| usize::from(slot))
            .filter(|&slot| slot < slots)
        {
            let (Some((variant, group)), Some(key)) =
                (world.slot(slot as u16), world.material_key(slot as u16))
            else {
                continue;
            };
            // A sprite has no view to read the scene from, so such a block keeps its Bedrock
            // sprite under a shader pack.
            if variant.features.scene {
                continue;
            }
            let id = *pipelines
                .ids
                .entry(variant)
                .or_insert_with(|| cache.queue_render_pipeline(descriptor(variant)));
            if let Some(pipeline) = cache.get_render_pipeline(id) {
                ready.push(ReadySprite {
                    slot,
                    key,
                    pipeline,
                    group,
                });
            }
        }
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shader-pack world material sprites"),
    });
    if sprites.encode(&queue, &mut encoder, &ready) {
        pending.push_encoder(encoder, "shader-pack world material sprites");
    }
}

#[cfg(test)]
#[path = "world_sprites_tests.rs"]
mod tests;
