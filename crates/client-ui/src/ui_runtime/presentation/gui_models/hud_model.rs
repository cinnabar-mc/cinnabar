//! Resident HUD model geometry, animated by small bone palettes in the shared UI pipeline.

use std::sync::{Arc, LazyLock};

use glam::{Affine3A, Mat3, Mat4, Vec3};
use render_model::{ActorRigVertex, ActorVertex, UiModelBone, UiModelPose};
use ui::{IconRef, UiMesh, UiMeshBatch, UiMeshVertex, UiNode};

use super::super::{
    UiPresentationRuntime,
    player_preview::{self, PreviewHeldModel, PreviewView, Rig},
};

/// One immutable material keeps its source allocation alive as an exact cache witness.
struct Source {
    vertices: Arc<[ActorRigVertex]>,
    bones: usize,
    drawn_bones: usize,
    palette_indices: Vec<u32>,
    page: u16,
    color_mask: bool,
    glint: bool,
    bounds: Vec<Option<(Vec3, Vec3)>>,
    effect: bool,
    hand: Option<usize>,
    visible: bool,
    projected: (Vec3, Vec3),
    material: (ui::UiBlendMode, bool, bool, Option<f32>),
}

#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct HudModel {
    sources: Vec<Source>,
    cursor: usize,
    dirty: bool,
    mesh: Option<Arc<UiMesh>>,
    bones: Vec<UiModelBone>,
    low: Vec3,
    high: Vec3,
    palette: Arc<[UiModelBone]>,
    spare_palettes: Vec<Arc<[UiModelBone]>>,
    poses: Arc<[UiModelPose]>,
    spare_poses: Vec<Arc<[UiModelPose]>>,
    pose_scratch: Vec<UiModelPose>,
    pose_bones: Vec<UiModelBone>,
    placed: Vec<Arc<[UiModelBone]>>,
    spare_placed: Vec<Arc<[UiModelBone]>>,
    fire_vertices: Vec<UiMeshVertex>,
    fire_batches: Vec<UiMeshBatch>,
}

impl HudModel {
    /// Resolves normalized model vertices only for CPU/GPU conformance assertions.
    #[cfg(test)]
    pub(in crate::ui_runtime::presentation) fn reference_vertices(
        &self,
        mesh: &UiMesh,
    ) -> Vec<UiMeshVertex> {
        mesh.vertices()
            .iter()
            .map(|vertex| {
                let projected = self.palette[vertex.clip_w as usize - 1].project(
                    render_model::UiRenderVertex {
                        position: vertex.position,
                        clip_z: vertex.clip_z,
                        clip_w: vertex.clip_w,
                        uv: vertex.uv,
                        color: vertex.color,
                        style_flags: u32::from(vertex.style_flags),
                        alpha_cutoff: -1.,
                        model_light: vertex.model_light,
                        overlay_color: vertex.overlay_color,
                    },
                );
                UiMeshVertex {
                    position: projected.position,
                    clip_z: projected.clip_z,
                    clip_w: projected.clip_w,
                    uv: projected.uv,
                    color: projected.color,
                    model_light: projected.model_light,
                    overlay_color: projected.overlay_color,
                    ..*vertex
                }
            })
            .collect()
    }

    /// Begins a palette sample while keeping the admitted mesh and source metadata.
    fn begin(&mut self) {
        self.cursor = 0;
        self.bones.clear();
        self.low = Vec3::splat(f32::INFINITY);
        self.high = Vec3::splat(f32::NEG_INFINITY);
    }

    /// Reuses a material's vertices and bounds while accepting new per-bone matrices and UVs.
    fn layer(
        &mut self,
        vertices: &Arc<[ActorRigVertex]>,
        frames: impl ExactSizeIterator<Item = Mat4>,
        light_frames: impl ExactSizeIterator<Item = Mat3>,
        icon: IconRef,
        tint: Option<[u8; 3]>,
        overlay: [f32; 4],
    ) -> Option<()> {
        if icon.uv[0] >= icon.uv[2] || icon.uv[1] >= icon.uv[3] {
            return None;
        }
        let count = frames.len();
        let index = self.cursor;
        self.cursor += 1;
        let matches = self.sources.get(index).is_some_and(|source| {
            Arc::ptr_eq(&source.vertices, vertices)
                && source.bones == count
                && source.page == icon.page
                && source.color_mask == tint.is_some()
                && source.glint == icon.glint
        });
        if !matches {
            let mut bounds: Vec<Option<(Vec3, Vec3)>> = vec![None; count];
            for vertex in vertices.iter() {
                let point = Vec3::from_array(vertex.position);
                let entry = bounds.get_mut(vertex.bone_index as usize)?;
                *entry = Some(entry.map_or((point, point), |(low, high)| {
                    (low.min(point), high.max(point))
                }));
            }
            let mut drawn_bones = 0;
            let palette_indices = bounds
                .iter()
                .map(|bounds| {
                    let index = drawn_bones;
                    if bounds.is_some() {
                        drawn_bones += 1;
                    }
                    index as u32
                })
                .collect();
            let source = Source {
                vertices: Arc::clone(vertices),
                bones: count,
                drawn_bones,
                palette_indices,
                page: icon.page,
                color_mask: tint.is_some(),
                glint: icon.glint,
                bounds,
                effect: false,
                hand: None,
                visible: true,
                projected: (Vec3::ZERO, Vec3::ZERO),
                material: (
                    ui::UiBlendMode::Alpha,
                    true,
                    true,
                    Some(if tint.is_some() { 1. / 255. } else { 0.5 }),
                ),
            };
            if index == self.sources.len() {
                self.sources.push(source);
            } else {
                self.sources[index] = source;
            }
            self.dirty = true;
        }
        let source = &mut self.sources[index];
        source.hand = None;
        let mut projected = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for ((screen, light), bounds) in frames.zip(light_frames).zip(&source.bounds) {
            let Some((low, high)) = bounds else { continue };
            {
                for corner in 0..8 {
                    let point = Vec3::new(
                        if corner & 1 == 0 { low.x } else { high.x },
                        if corner & 2 == 0 { low.y } else { high.y },
                        if corner & 4 == 0 { low.z } else { high.z },
                    );
                    let point = screen.transform_point3(point);
                    projected.0 = projected.0.min(point);
                    projected.1 = projected.1.max(point);
                }
            }
            let rows = light.transpose().to_cols_array_2d();
            self.bones.push(UiModelBone {
                screen: screen.to_cols_array_2d(),
                normal: rows.map(|row| [row[0], row[1], row[2], 0.0]),
                overlay,
                uv_rect: icon.uv.map(f32::from),
                styles: [0; 4],
                uv_cross: [0.; 4],
                tint: {
                    let color = tint.unwrap_or([255; 3]);
                    [
                        f32::from(color[0]) / 255.,
                        f32::from(color[1]) / 255.,
                        f32::from(color[2]) / 255.,
                        1.,
                    ]
                },
            });
        }
        source.projected = projected;
        Some(())
    }

    /// Keeps flame and particle triangles in the same depth scope without copying the body mesh.
    fn effect(
        &mut self,
        vertices: &[UiMeshVertex],
        indices: Option<&[u32]>,
        material: &UiMeshBatch,
    ) -> Option<()> {
        let start = material.index_range.start as usize;
        let count = (material.index_range.end as usize - start) / 3;
        if count == 0 {
            return Some(());
        }
        let triangle = |index: usize| -> [UiMeshVertex; 3] {
            std::array::from_fn(|corner| {
                let offset = start + index * 3 + corner;
                vertices[indices.map_or(offset, |indices| indices[offset] as usize)]
            })
        };
        let old_capacity = self
            .sources
            .get(self.cursor)
            .filter(|source| source.effect)
            .map_or(0, |source| source.bones);
        let other_vertices: usize = self
            .sources
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.cursor)
            .map(|(_, source)| source.vertices.len())
            .sum();
        let available = ui::UiLimits::MAX_UI_VERTICES.saturating_sub(other_vertices) / 3;
        let capacity = old_capacity.max(count.next_power_of_two().min(available).max(count));
        let template = self
            .sources
            .get(self.cursor)
            .filter(|source| source.effect && source.bones == capacity)
            .map(|source| Arc::clone(&source.vertices))
            .unwrap_or_else(|| {
                (0..capacity)
                    .flat_map(|bone| {
                        [Vec3::ZERO, Vec3::X, Vec3::Y].map(|position| ActorRigVertex {
                            position: position.to_array(),
                            uv: [position.x, position.y],
                            bone_index: bone as u32,
                            ..Default::default()
                        })
                    })
                    .collect::<Vec<_>>()
                    .into()
            });
        let base = self.bones.len();
        self.layer(
            &template,
            (0..capacity).map(|index| {
                let points = triangle(index.min(count - 1))
                    .map(|vertex| Vec3::new(vertex.position[0], vertex.position[1], vertex.clip_z));
                if index < count {
                    Mat4::from_cols(
                        (points[1] - points[0]).extend(0.),
                        (points[2] - points[0]).extend(0.),
                        Vec3::Z.extend(0.),
                        points[0].extend(1.),
                    )
                } else {
                    Mat4::from_cols(
                        glam::Vec4::ZERO,
                        glam::Vec4::ZERO,
                        glam::Vec4::ZERO,
                        points[0].extend(1.),
                    )
                }
            }),
            std::iter::repeat_n(Mat3::ZERO, capacity),
            IconRef {
                page: material.texture_page,
                uv: [0, 0, 1, 1],
                color_mask: None,
                glint: false,
            },
            None,
            [0.; 4],
        )?;
        let source = &mut self.sources[self.cursor - 1];
        let properties = (
            material.blend,
            material.depth_test,
            material.depth_write,
            material.alpha_cutoff,
        );
        self.dirty |= source.material != properties || !source.effect;
        source.material = properties;
        source.effect = true;
        for index in 0..count {
            let corners = triangle(index);
            let bone = &mut self.bones[base + index];
            bone.uv_rect = [
                corners[0].uv[0],
                corners[0].uv[1],
                corners[1].uv[0],
                corners[2].uv[1],
            ];
            bone.uv_cross = [
                corners[2].uv[0] - corners[0].uv[0],
                corners[1].uv[1] - corners[0].uv[1],
                0.,
                0.,
            ];
            bone.tint = corners[0].color.map(|channel| f32::from(channel) / 255.);
            bone.overlay = corners[0].overlay_color;
            bone.styles[0] = u32::from(corners[0].style_flags)
                | if corners[0].alpha_test {
                    render_api::UI_STYLE_ALPHA_TEST
                } else {
                    0
                };
        }
        Some(())
    }

    /// Admits static topology once and normalizes the shared reverse-Z interval on the palette.
    fn finish(
        &mut self,
        mut admit: impl FnMut(&Self, [bool; 2], usize, usize) -> bool,
    ) -> Option<Arc<UiMesh>> {
        if self.cursor != self.sources.len() {
            self.sources.truncate(self.cursor);
            self.dirty = true;
        }
        let mask = [[true; 2], [true, false], [false, true], [false; 2]]
            .into_iter()
            .find(|mask| {
                let (indices, batches) = self
                    .sources
                    .iter()
                    .filter(|source| source.hand.is_none_or(|hand| mask[hand]))
                    .fold((0, 0), |(indices, batches), source| {
                        (
                            indices + source.vertices.len(),
                            batches + usize::from(!source.vertices.is_empty()),
                        )
                    });
                UiMesh::check_size(indices, indices, batches).is_ok()
                    && admit(self, *mask, indices, batches)
            })?;
        for source in &mut self.sources {
            let visible = source.hand.is_none_or(|hand| mask[hand]);
            self.dirty |= source.visible != visible;
            source.visible = visible;
            if visible {
                self.low = self.low.min(source.projected.0);
                self.high = self.high.max(source.projected.1);
            }
        }
        let span = self.high.z - self.low.z;
        if !span.is_finite() || span <= 0.0 {
            return None;
        }
        let depth = Mat4::from_translation(Vec3::new(0., 0., 0.25 - self.low.z / span * 0.5))
            * Mat4::from_scale(Vec3::new(1., 1., 0.5 / span));
        for bone in &mut self.bones {
            bone.screen = (depth * Mat4::from_cols_array_2d(&bone.screen)).to_cols_array_2d();
        }
        if self.dirty || self.mesh.is_none() {
            let count: usize = self
                .sources
                .iter()
                .filter(|source| source.visible)
                .map(|source| source.vertices.len())
                .sum();
            UiMesh::check_size(count, count, self.sources.len()).ok()?;
            let mut vertices = Vec::with_capacity(count);
            let mut batches = Vec::with_capacity(self.sources.len());
            let mut base = 0;
            for source in &self.sources {
                if !source.visible {
                    base += source.drawn_bones as u32;
                    continue;
                }
                let start = vertices.len() as u32;
                for vertex in source.vertices.iter() {
                    vertices.push(UiMeshVertex {
                        position: [vertex.position[0], vertex.position[1]],
                        clip_z: vertex.position[2],
                        clip_w: (base + source.palette_indices[vertex.bone_index as usize] + 1)
                            as f32,
                        uv: vertex.uv,
                        color: [255; 4],
                        model_light: 1.0,
                        overlay_color: [vertex.normal[0], vertex.normal[1], vertex.normal[2], 0.0],
                        style_flags: if source.glint {
                            u16::from(ui::UI_STYLE_GLINT)
                        } else {
                            0
                        } | if source.color_mask {
                            render_model::UI_STYLE_COLOR_MASK as u16
                        } else {
                            0
                        },
                        alpha_test: false,
                    });
                }
                let end = vertices.len() as u32;
                if start != end {
                    batches.push(UiMeshBatch {
                        texture_page: source.page,
                        index_range: start..end,
                        blend: source.material.0,
                        depth_test: source.material.1,
                        depth_write: source.material.2,
                        alpha_cutoff: source.material.3,
                    });
                }
                base += source.drawn_bones as u32;
            }
            let indices: Vec<_> = (0..vertices.len() as u32).collect();
            self.mesh = Some(Arc::new(
                UiMesh::skinned(vertices.into(), indices.into(), batches.into()).ok()?,
            ));
            self.dirty = false;
        }
        reuse_slice(&mut self.palette, &mut self.spare_palettes, &self.bones);
        self.mesh.clone()
    }

    /// Probes actual batch coalescing only when the conservative frame bound is exceeded.
    pub(super) fn batch_probe(&self, mask: [bool; 2]) -> Option<Arc<UiMesh>> {
        let mut vertices = Vec::new();
        let mut batches = Vec::new();
        for source in self.sources.iter().filter(|source| {
            !source.vertices.is_empty() && source.hand.is_none_or(|hand| mask[hand])
        }) {
            let start = vertices.len() as u32;
            vertices.extend(
                [UiMeshVertex {
                    position: [0.; 2],
                    clip_z: 0.5,
                    clip_w: 1.,
                    uv: [0.; 2],
                    color: [255; 4],
                    model_light: 1.,
                    overlay_color: [0.; 4],
                    style_flags: 0,
                    alpha_test: false,
                }; 3],
            );
            batches.push(UiMeshBatch {
                texture_page: source.page,
                index_range: start..start + 3,
                blend: source.material.0,
                depth_test: source.material.1,
                depth_write: source.material.2,
                alpha_cutoff: source.material.3,
            });
        }
        let indices = (0..vertices.len() as u32).collect::<Vec<_>>();
        Some(Arc::new(
            UiMesh::new(vertices.into(), indices.into(), batches.into()).ok()?,
        ))
    }

    /// Places the small palette using the retained control's actual laid-out bounds.
    pub(in crate::ui_runtime::presentation) fn poses(
        &mut self,
        draw: &ui::RetainedDraw,
        nodes: &[UiNode],
        dpi: f32,
        viewport: [u32; 2],
    ) -> Arc<[UiModelPose]> {
        let mut poses = std::mem::take(&mut self.pose_scratch);
        poses.clear();
        for (index, node) in nodes.iter().enumerate() {
            let (mesh, color) = match node.visual() {
                ui::UiVisual::Mesh(mesh) => (mesh, [255; 4]),
                ui::UiVisual::TintedMesh { mesh, color } => (mesh, *color),
                _ => continue,
            };
            if !mesh.is_skinned() {
                continue;
            }
            let bounds = draw.node_bounds(index).expect("retained model placement");
            let mut placement =
                Mat4::from_translation(Vec3::new(bounds.min().x(), bounds.min().y(), 0.))
                    * Mat4::from_scale(Vec3::new(bounds.width(), bounds.height(), 1.));
            if let Some(transform) = node.transform() {
                let [a, b, c, d] = transform.linear;
                let origin = Vec3::new(bounds.min().x(), bounds.min().y(), 0.);
                let linear = Mat4::from_cols(
                    Vec3::new(a, b, 0.).extend(0.),
                    Vec3::new(c, d, 0.).extend(0.),
                    Vec3::Z.extend(0.),
                    glam::Vec4::W,
                );
                placement = Mat4::from_translation(
                    origin + Vec3::new(transform.translation[0], transform.translation[1], 0.),
                ) * linear
                    * Mat4::from_translation(-origin)
                    * placement;
            }
            placement = Mat4::from_scale(Vec3::new(dpi, dpi, 1.)) * placement;
            let mut bones = std::mem::take(&mut self.pose_bones);
            bones.clear();
            bones.extend_from_slice(&self.palette);
            for bone in &mut bones {
                bone.tint = [0, 1, 2, 3].map(|channel| {
                    let source = (bone.tint[channel] * 255.).round() as u16;
                    f32::from((source * u16::from(color[channel]) / 255) as u8) / 255.
                });
                bone.screen =
                    (placement * Mat4::from_cols_array_2d(&bone.screen)).to_cols_array_2d();
            }
            // Every palette is small; the frame pool retains its allocation across publications.
            let position = poses.len();
            if position == self.placed.len() {
                self.placed.push(Arc::from([]));
            }
            reuse_slice(&mut self.placed[position], &mut self.spare_placed, &bones);
            self.pose_bones = bones;
            let bones = Arc::clone(&self.placed[position]);
            let coverage = [
                Vec3::new(self.low.x, self.low.y, 0.),
                Vec3::new(self.high.x, self.low.y, 0.),
                Vec3::new(self.low.x, self.high.y, 0.),
                Vec3::new(self.high.x, self.high.y, 0.),
            ]
            .map(|point| placement.transform_point3(point));
            let low = coverage
                .into_iter()
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let high = coverage
                .into_iter()
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            let left = (low.x.floor() - 1.).max(0.).min(viewport[0] as f32) as u32;
            let top = (low.y.floor() - 1.).max(0.).min(viewport[1] as f32) as u32;
            let right = (high.x.ceil() + 1.).max(0.).min(viewport[0] as f32) as u32;
            let bottom = (high.y.ceil() + 1.).max(0.).min(viewport[1] as f32) as u32;
            poses.push(UiModelPose {
                scope: node.id().get(),
                bones,
                bounds: render_model::UiScissor::new(
                    left.min(viewport[0]),
                    top.min(viewport[1]),
                    right.min(viewport[0]).saturating_sub(left),
                    bottom.min(viewport[1]).saturating_sub(top),
                ),
            });
        }
        reuse_slice(&mut self.poses, &mut self.spare_poses, &poses);
        self.pose_scratch = poses;
        Arc::clone(&self.poses)
    }
}

/// Recycles immutable publication storage after every consumer releases its earlier frame.
fn reuse_slice<T: Clone + PartialEq>(
    current: &mut Arc<[T]>,
    spares: &mut Vec<Arc<[T]>>,
    values: &[T],
) {
    if current.as_ref() == values {
        return;
    }
    if let Some(index) = spares.iter().position(|spare| {
        spare.len() == values.len() && Arc::strong_count(spare) == 1 && Arc::weak_count(spare) == 0
    }) {
        Arc::get_mut(&mut spares[index])
            .expect("unshared frame storage")
            .clone_from_slice(values);
        std::mem::swap(current, &mut spares[index]);
    } else {
        spares.push(std::mem::replace(current, Arc::from(values)));
    }
}

/// Samples the same affine view projection used by the CPU conformance renderer.
fn projection(rig: &Rig, part: u32) -> (Mat4, Mat3) {
    let sample = |point: Vec3| {
        rig.project(ActorVertex {
            position: point.to_array(),
            uv: [0.; 2],
            part,
        })
    };
    let origin = sample(Vec3::ZERO);
    let axes = [Vec3::X, Vec3::Y, Vec3::Z].map(sample);
    let normalized = |vertex: &player_preview::ProjectedVertex| {
        Vec3::new(
            vertex.screen[0] / player_preview::PREVIEW_WIDTH as f32,
            vertex.screen[1] / player_preview::PREVIEW_HEIGHT as f32,
            vertex.world[2],
        )
    };
    let at = normalized(&origin);
    let vectors = axes.each_ref().map(|point| normalized(point) - at);
    let screen = Mat4::from_cols(
        vectors[0].extend(0.),
        vectors[1].extend(0.),
        vectors[2].extend(0.),
        at.extend(1.),
    );
    let world_axes = axes
        .each_ref()
        .map(|point| Vec3::from_array(point.world) - Vec3::from_array(origin.world));
    let world = Mat3::from_cols(world_axes[0], world_axes[1], world_axes[2]);
    (screen, world)
}

/// Keeps finite area normals, including faces that survive a flattened bone scale.
fn normal_frame(frame: Mat3, outward: bool) -> Mat3 {
    let scale = frame
        .x_axis
        .abs()
        .max_element()
        .max(frame.y_axis.abs().max_element())
        .max(frame.z_axis.abs().max_element());
    if scale == 0.0 {
        return Mat3::ZERO;
    }
    let unit = Mat3::from_cols_array(&frame.to_cols_array().map(|value| value / scale));
    let normal = Mat3::from_cols(
        unit.y_axis.cross(unit.z_axis),
        unit.z_axis.cross(unit.x_axis),
        unit.x_axis.cross(unit.y_axis),
    );
    if outward && unit.determinant() < 0.0 {
        -normal
    } else {
        normal
    }
}

/// Converts fixed biped equipment once, preserving the CPU renderer's outward face normals.
fn armor_source(slot: usize) -> Arc<[ActorRigVertex]> {
    static SOURCES: LazyLock<[Arc<[ActorRigVertex]>; 4]> = LazyLock::new(|| {
        std::array::from_fn(|slot| {
            let vertices = player_preview::equipment::armor_vertices(slot);
            let centers = player_preview::geometry::lighting::part_centers(&vertices);
            vertices
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|triangle| {
                    let points = triangle.map(|vertex| Vec3::from_array(vertex.position));
                    let mut normal = (points[1] - points[0])
                        .cross(points[2] - points[0])
                        .normalize();
                    let center = Vec3::from_array(centers[&triangle[0].part]);
                    if normal.dot((points[0] + points[1] + points[2]) / 3. - center) < 0. {
                        normal = -normal;
                    }
                    triangle.map(|vertex| ActorRigVertex {
                        position: vertex.position,
                        normal: normal.to_array(),
                        uv: vertex.uv,
                        bone_index: vertex.part,
                        ..Default::default()
                    })
                })
                .collect::<Vec<_>>()
                .into()
        })
    });
    Arc::clone(&SOURCES[slot])
}

impl UiPresentationRuntime {
    /// Draws the live actor from resident source vertices and freshly evaluated bone frames.
    pub(super) fn hud_player_mesh(
        &self,
        held: [Option<PreviewHeldModel>; 2],
        admit: impl FnMut(&HudModel, [bool; 2], usize, usize) -> bool,
    ) -> Option<Arc<UiMesh>> {
        let live = &self.gui_models.live_player;
        let skin = self.gui_models.skin.as_ref()?;
        let [width, height] = skin.dimensions();
        let icon = IconRef {
            color_mask: None,
            page: (self.textures.dynamic_start() + super::SKIN_PAGE) as u16,
            uv: [0, 0, width.try_into().ok()?, height.try_into().ok()?],
            glint: false,
        };
        let mut cache = self
            .gui_models
            .hud_model
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        cache.begin();
        let mut rig = Rig::new(
            self.player_preview_pose.unwrap_or_default(),
            PreviewView::Hud,
            self.player_preview_bob,
            held.each_ref().map(Option::is_some),
        );
        rig.parts = live.parts;
        let (screen, light) = projection(&rig, 6);
        let mut model = |model: &super::live_player::ModelGeometry, icon: IconRef| -> Option<()> {
            let geometry = model.geometry.as_ref()?;
            cache.layer(
                &geometry.vertices,
                model
                    .transforms
                    .iter()
                    .map(|frame| screen * Mat4::from(*frame)),
                model
                    .transforms
                    .iter()
                    .map(|frame| normal_frame(light * Mat3::from(frame.matrix3), true)),
                icon,
                None,
                live.overlay_color,
            )
        };
        model(&live.body, icon)?;
        if self.menu_preview_model.cape_key.is_some()
            && !self.gui_models.wearing_elytra
            && let Some(icon) = self
                .menu_preview_model
                .cape_key
                .as_ref()
                .and_then(|key| self.menu_artwork_icon(key))
            && live.cape.geometry.is_some()
        {
            model(&live.cape, icon)?;
        }
        for item in &self.menu_preview_model.cosmetics {
            if let Some(model_source) = live
                .hats
                .get(item)
                .filter(|source| source.geometry.is_some())
                && let Some(icon) = model_source
                    .texture_key
                    .as_deref()
                    .and_then(|key| self.menu_artwork_icon(key))
            {
                model(model_source, icon)?;
            }
        }
        for slot in 0..4 {
            let Some(texture) = &self.player_preview_gear.armor[slot] else {
                continue;
            };
            let icon = self.gui_models.pack_equipment.region(slot).or_else(|| {
                self.gui_models
                    .sources
                    .textures
                    .get(&super::atlas::key(
                        [texture.width, texture.height],
                        &texture.rgba,
                    ))
                    .copied()
            });
            let Some(icon) = icon else { continue };
            let frames = [0, 1, 2, 3, 4, 5].map(|part| projection(&rig, part));
            cache.layer(
                &armor_source(slot),
                frames.iter().map(|frame| frame.0),
                frames.iter().map(|frame| normal_frame(frame.1, false)),
                icon.with_glint(self.player_preview_gear.armor_glint[slot]),
                texture.tint,
                live.overlay_color,
            )?;
        }
        for (hand, model) in held.iter().enumerate() {
            let Some(model) = model else { continue };
            let (bone, pivot) = player_preview::geometry::held::placement(model, hand)?;
            let rotation = glam::Quat::from_array(bone.rotation).normalize();
            let scale = Vec3::from_slice(&bone.axis_scale[..3]) * bone.translation_scale[3];
            let frame = Affine3A::from_scale(Vec3::new(-1., 1., -1.))
                * Affine3A::from_scale_rotation_translation(
                    scale,
                    rotation,
                    Vec3::from_slice(&bone.translation_scale[..3]),
                )
                * Affine3A::from_translation(-pivot);
            let (screen, light) = projection(&rig, if hand == 0 { 2 } else { 3 });
            cache.layer(
                &model.vertices,
                std::iter::once(screen * Mat4::from(frame)),
                std::iter::once(light * Mat3::from(frame.matrix3)),
                model.source,
                None,
                [0.; 4],
            )?;
            let index = cache.cursor - 1;
            cache.sources[index].hand = Some(hand);
        }
        if let Some(fire) = self.gui_fire_frame(PreviewView::Hud) {
            let mut vertices = std::mem::take(&mut cache.fire_vertices);
            let mut batches = std::mem::take(&mut cache.fire_batches);
            vertices.clear();
            batches.clear();
            let prepared =
                player_preview::geometry::fire::append(&mut vertices, &mut batches, fire).and_then(
                    |()| {
                        batches
                            .iter()
                            .try_for_each(|batch| cache.effect(&vertices, None, batch))
                    },
                );
            cache.fire_vertices = vertices;
            cache.fire_batches = batches;
            prepared?;
        }
        if self.player_preview_view == PreviewView::Hud
            && let Some(particles) = self.gui_models.particles.mesh.as_deref()
        {
            for batch in particles.batches() {
                cache.effect(particles.vertices(), Some(particles.indices()), batch)?;
            }
        }
        cache.finish(admit)
    }
}
