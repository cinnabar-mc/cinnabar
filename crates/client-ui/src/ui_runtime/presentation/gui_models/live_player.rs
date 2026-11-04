//! The HUD uses the same geometry and evaluated bones as the world player.

use super::super::UiPresentationRuntime;
use glam::{Affine3A, Mat3, Quat, Vec3, Vec4};
#[cfg(any(test, feature = "test-support"))]
use render_model::ActorVertex;
use render_model::{ActorRigGeometry, EntityRigId, RenderBoneTransform};

type GeometryIdentity = (u32, Option<[u8; 32]>, Option<[u8; 32]>);

#[derive(Default)]
pub(super) struct ModelGeometry {
    pub(super) geometry: Option<ActorRigGeometry>,
    pub(super) transforms: Vec<Affine3A>,
    pub(super) texture_key: Option<String>,
    /// A cosmetic anchored to the head turns with it in a screen's live renderer.
    pub(super) follows_head: bool,
}

impl ModelGeometry {
    /// Retains immutable geometry and evaluates each bone frame once, independently of vertex count.
    pub(super) fn capture(
        &mut self,
        geometry: &ActorRigGeometry,
        pose: &[RenderBoneTransform],
        frame: Affine3A,
    ) {
        self.transforms.clear();
        for (index, pivot) in geometry.bone_pivots.iter().enumerate() {
            if geometry.bone_bounds(index).is_none() {
                self.transforms.push(Affine3A::IDENTITY);
                continue;
            }
            let Some(bone) = pose.get(index).and_then(render_matrix) else {
                self.geometry = None;
                return;
            };
            self.transforms
                .push(frame * bone * Affine3A::from_translation(-Vec3::from_array(*pivot)));
        }
        self.geometry = Some(geometry.clone());
    }

    /// Materializes a CPU witness only when an offline conformance test requests it.
    #[cfg(test)]
    fn reference(&self, output: &mut Vec<ActorVertex>) {
        output.clear();
        let Some(geometry) = &self.geometry else {
            return;
        };
        output.extend(geometry.vertices.iter().map(|vertex| {
            ActorVertex {
                position: self.transforms[vertex.bone_index as usize]
                    .transform_point3(Vec3::from_array(vertex.position))
                    .to_array(),
                uv: vertex.uv,
                part: vertex.bone_index + 6,
            }
        }));
    }
}

#[derive(Default)]
pub(super) struct LivePlayer {
    source: Option<GeometryIdentity>,
    // Retain the allocation so its address cannot be reused as a cache identity.
    skin: Option<std::sync::Arc<assets::SkinGeometry>>,
    geometry: Option<ActorRigGeometry>,
    pub(super) body: ModelGeometry,
    pub(super) cape: ModelGeometry,
    pub(super) hats: std::collections::HashMap<cosmetics::Item, ModelGeometry>,
    #[cfg(test)]
    pub(super) vertices: Vec<ActorVertex>,
    #[cfg(test)]
    pub(super) cape_vertices: Vec<ActorVertex>,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) hat_vertices: std::collections::HashMap<cosmetics::Item, Vec<ActorVertex>>,
    hat_pose: std::collections::HashMap<cosmetics::Item, render_model::hats::HatPoseCache>,
    pub(super) hat_motion: std::collections::HashMap<cosmetics::Item, render_model::hats::Motion>,
    pub(super) bones: Vec<RenderBoneTransform>,
    cape_bones: Vec<RenderBoneTransform>,
    cape_bindings: Option<(
        GeometryIdentity,
        [u8; 32],
        Vec<render_model::CapeBoneBinding>,
    )>,
    pub(super) emitter_owner: Option<(u64, u64, u64)>,
    pub(super) emitters: Vec<(cosmetics::Item, usize, render_model::ActorEmitterTransform)>,
    pub(super) parts: [Option<Affine3A>; 6],
    /// Unmasked item bones from the UI evaluation, independent of arm visibility.
    pub(super) hands: [Option<RenderBoneTransform>; 2],
    pub(super) frame: Affine3A,
    part_bindings: [Option<(usize, Affine3A)>; 6],
    pub(super) fire_size: Option<[f32; 2]>,
    pub(super) outer_y: f32,
    /// `[pitch, yaw]` of the gliding body in degrees, applied about the feet.
    pub(super) glide: Option<[f32; 2]>,
    /// Whether this frame's publication captured the actor, so a screen may draw it.
    pub(super) fresh: bool,
    /// The captured head bone, from which a live renderer turns it.
    head: Option<CapturedHead>,
    /// Which body bones follow the head.
    pub(super) head_bones: Vec<bool>,
    pub(super) overlay_color: [f32; 4],
    pub(super) fire: super::fire::FirePlayback,
}

/// The head bone in model space, kept apart from the model frame so that a frame's
/// non-uniform scale never has to be decomposed.
#[derive(Clone, Copy)]
struct CapturedHead {
    /// Model space to model UI space, as applied to every captured bone.
    frame: Affine3A,
    /// The frame's rotation alone: the body's facing and glide tilt.
    facing: Mat3,
    pivot: Vec3,
    rotation: Quat,
}

impl LivePlayer {
    /// The transform that replaces the captured head orientation with `pitch` and `yaw` radians
    /// about the head pivot, in the same space as the captured bone frames.
    pub(super) fn head_turn(&self, pitch: f32, yaw: f32) -> Option<Affine3A> {
        let head = self.head?;
        // The preview rig's Y turn runs opposite glam's for the same angle. The look is given
        // in UI space and brought into the body's own space.
        let look = head.facing.transpose()
            * Mat3::from_rotation_y(-yaw)
            * Mat3::from_rotation_x(pitch)
            * head.facing;
        let turn = Affine3A::from_translation(head.pivot)
            * Affine3A::from_mat3(look * Mat3::from_quat(head.rotation.inverse()))
            * Affine3A::from_translation(-head.pivot);
        Some(head.frame * turn * head.frame.inverse())
    }
}

impl UiPresentationRuntime {
    /// Marks the next frame's actor capture as not yet taken; call before capturing.
    pub fn begin_live_capture(&mut self) {
        self.gui_models.live_player.fresh = false;
        self.gui_models.player_layers.clear_capture();
    }

    /// Whether a screen drew a live player renderer in the last published frame.
    pub fn live_renderer_shown(&self) -> bool {
        self.gui_models
            .live_renderer_shown
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Shares the actor frame's evaluated motion without advancing a second animation clock.
    pub fn set_cosmetic_motion(
        &mut self,
        motion: &[(cosmetics::Item, render_model::hats::Motion)],
    ) {
        let retained = &mut self.gui_models.live_player.hat_motion;
        if retained.len() == motion.len()
            && motion
                .iter()
                .all(|(item, value)| retained.get(item) == Some(value))
        {
            return;
        }
        retained.retain(|item, _| motion.iter().any(|(active, _)| active == item));
        for &(item, value) in motion {
            retained.insert(item, value);
        }
        // The next capture updates bone palettes without discarding the retained HUD.
    }

    /// Captures full-body bone frames, never the separate first-person hand skeleton.
    pub fn capture_hud_player(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
        swimming: bool,
    ) {
        self.capture_hud_player_with_emote(local, swimming, None);
    }

    /// Captures an optional local emote without changing the world rig or hand.
    /// Sets the gliding body rotation for later captures; `None` stands the doll upright.
    pub fn set_hud_glide(&mut self, rotation: Option<[f32; 2]>) {
        self.gui_models.live_player.glide = rotation;
    }

    /// Captures the UI pose and both renderer contexts from the same completed actor tick.
    pub fn capture_hud_player_with_emote(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
        swimming: bool,
        emote: Option<(cosmetics::emotes::Item, f64)>,
    ) {
        self.capture_live_pose(local, swimming, emote);
        self.capture_player_layers(local);
        self.capture_live_equipment(local);
    }

    /// Retains the animated body and unmasked attachment bones for the current capture.
    fn capture_live_pose(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
        swimming: bool,
        emote: Option<(cosmetics::emotes::Item, f64)>,
    ) {
        let fire_frames = self.gui_models.fire.frames.len();
        let draw_cape =
            self.menu_preview_model.cape_key.is_some() && !self.gui_models.wearing_elytra;
        let live = &mut self.gui_models.live_player;
        #[cfg(test)]
        {
            live.vertices.clear();
            live.cape_vertices.clear();
            live.hat_vertices.retain(|item, vertices| {
                vertices.clear();
                self.menu_preview_model.cosmetics.contains(item)
            });
        }
        live.emitters.clear();
        live.emitter_owner = None;
        live.fresh = false;
        live.head = None;
        live.parts = [None; 6];
        live.hands = [None; 2];
        live.frame = Affine3A::IDENTITY;
        live.fire_size = None;
        live.body.geometry = None;
        live.cape.geometry = None;
        live.hats.retain(|item, model| {
            model.geometry = None;
            self.menu_preview_model.cosmetics.contains(item)
        });
        live.outer_y = if swimming {
            super::super::player_preview::HUD_SWIM_OFFSET
        } else {
            0.0
        };
        live.overlay_color = [0.0; 4];
        let Some(local) = local else {
            live.source = None;
            live.fire = Default::default();
            return;
        };
        live.fresh = true;
        let id = local.runtime_id();
        live.emitter_owner = local
            .actor()
            .map(|actor| (local.session_id(), id, actor.spawn_revision));
        live.fire_size = local.actor().and_then(|actor| {
            live.fire.observe(
                (local.session_id(), id, actor.spawn_revision),
                actor.is_on_fire(),
                fire_frames,
            );
            live.overlay_color = super::fire::native_player_overlay(actor);
            if !actor.is_on_fire() {
                return None;
            }
            let (min, max) = actor.bounding_box()?;
            Some([max[0] - min[0], max[1] - min[1]])
        });
        let Some(rig) = local.rig() else {
            return;
        };
        let emote_pose = emote.and_then(|(emote, phase)| {
            actor_animation::sample_custom_emote(&rig, emote, phase, phase)
        });
        let rig = emote_pose.as_ref().map_or(rig, |pose| pose.snapshot(rig));
        let Some(pose) = emote_pose
            .as_ref()
            .map(|pose| pose.current.as_ref())
            .or_else(|| local.pose())
        else {
            return;
        };
        let basis = Affine3A::from_scale(Vec3::new(-1., 1., -1.));
        let tilt = live.glide.map_or(Affine3A::IDENTITY, |[pitch, yaw]| {
            Affine3A::from_rotation_x(pitch.to_radians())
                * Affine3A::from_rotation_y(yaw.to_radians())
        });
        let swim_offset = live.outer_y / super::super::player_preview::PLAYER_MODEL_SCALE;
        let offset = Affine3A::from_translation(Vec3::Y * swim_offset);
        let actor_scale = Affine3A::from_scale(
            Vec3::from_array(rig.axis_scale)
                * (rig.scale / super::super::player_preview::PLAYER_MODEL_SCALE),
        );
        let catalog = rig.geometry_source();
        let source = (
            rig.rig.0,
            rig.skin_geometry.map(|skin| skin.digest),
            catalog.map(|(assets, _)| assets.source_manifest_sha256()),
        );
        if live.source != Some(source) {
            live.geometry = if let Some(skin) = rig.skin_geometry {
                render_model::skin_geometry(skin, EntityRigId(rig.rig.0)).ok()
            } else {
                catalog.and_then(|(assets, geometry)| {
                    render_model::entity_geometry(assets, geometry, EntityRigId(rig.rig.0)).ok()
                })
            };
            live.source = Some(source);
            live.skin = rig.skin_geometry.cloned();
            live.part_bindings = render_model::STANDARD_BIPED_BONES.map(|name| {
                let index = rig
                    .bone_names
                    .iter()
                    .position(|bone| bone.as_ref() == name)?;
                Some((index, bone_matrix(rig.rest.get(index)?)?.inverse()))
            });
        }
        for (part, binding) in live.part_bindings.into_iter().enumerate() {
            if let Some((index, rest_inverse)) = binding
                && let Some(posed) = pose.get(index).and_then(bone_matrix)
            {
                live.parts[part] =
                    Some(offset * actor_scale * basis * tilt * posed * rest_inverse * basis);
            }
        }
        let Some(geometry) = &live.geometry else {
            return;
        };
        live.bones.clear();
        for bone in pose {
            let Some(bone) = render_bone(bone) else {
                return;
            };
            live.bones.push(bone);
        }
        let frame = offset * actor_scale * basis * tilt;
        live.frame = frame;
        live.hands = ["rightItem", "leftItem"].map(|name| {
            let index = rig
                .bone_names
                .iter()
                .position(|bone| bone.eq_ignore_ascii_case(name))?;
            live.bones.get(index).copied()
        });
        live.body.capture(geometry, &live.bones, frame);
        let head_index = live.part_bindings[0].map(|(index, _)| index);
        live.head = head_index.and_then(|index| {
            let bone = live.bones.get(index)?;
            let rotation = Vec4::from_array(bone.rotation)
                .try_normalize()
                .map(Quat::from_vec4)?;
            // A model scaled to nothing has no frame to turn within.
            (frame.matrix3.determinant().abs() > f32::EPSILON).then_some(CapturedHead {
                frame,
                facing: Mat3::from((basis * tilt).matrix3),
                pivot: Vec3::new(
                    bone.translation_scale[0],
                    bone.translation_scale[1],
                    bone.translation_scale[2],
                ),
                rotation,
            })
        });
        live.head_bones.clear();
        if let Some(head) = head_index {
            live.head_bones
                .extend((0..live.bones.len()).map(|bone| local.bone_descends_from(bone, head)));
        }
        live.hat_pose
            .retain(|item, _| self.menu_preview_model.cosmetics.contains(item));
        for &hat in &self.menu_preview_model.cosmetics {
            if render_model::hats::rig(hat).is_ground_effect() {
                continue;
            }
            let cache = live.hat_pose.entry(hat).or_default();
            let Some(index) = cache.anchor(hat, rig.attachments, rig.bone_names) else {
                continue;
            };
            let Some(head) = live.bones.get(index) else {
                continue;
            };
            let pose = cache.pose(
                hat,
                *head,
                live.hat_motion.get(&hat).copied().unwrap_or_default(),
            );
            let world = frame.matrix3.to_cols_array_2d();
            let translation = frame.translation.to_array();
            let world = std::array::from_fn(|row| {
                [
                    world[0][row],
                    world[1][row],
                    world[2][row],
                    translation[row],
                ]
            });
            for index in 0..render_model::hats::rig(hat).emitters().len() {
                if let Some(transform) =
                    render_model::hats::rig(hat).emitter_transform(index, &pose, &pose, world, 1.0)
                {
                    live.emitters.push((hat, index, transform));
                }
            }
            let model = live.hats.entry(hat).or_insert_with(|| ModelGeometry {
                texture_key: Some(hat.texture_key()),
                ..Default::default()
            });
            model.capture(&render_model::hats::rig(hat).geometry, &pose, frame);
            model.follows_head = live.head_bones.get(index).copied().unwrap_or(false);
        }
        if draw_cape && let Some(cape) = &self.gui_models.sources.cape {
            let bindings = if let Some(bindings) =
                rig.attachments.and_then(|bindings| bindings.cape_for(cape))
            {
                bindings
            } else {
                if live
                    .cape_bindings
                    .as_ref()
                    .is_none_or(|(body, key, _)| *body != source || *key != cape.binding_key())
                {
                    live.cape_bindings =
                        Some((source, cape.binding_key(), cape.bind(rig.bone_names)));
                }
                &live.cape_bindings.as_ref().expect("bound cape source").2
            };
            cape.pose_bound_into(
                bindings,
                |index| rig.rest.get(index).and_then(render_bone),
                &live.bones,
                None,
                &mut live.cape_bones,
            );
            live.cape.capture(&cape.geometry, &live.cape_bones, frame);
        }
    }

    /// Materializes the pre-palette CPU witness for the existing geometry conformance tests.
    #[cfg(test)]
    fn capture_hud_player_reference(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
        swimming: bool,
    ) {
        self.capture_hud_player_with_emote_reference(local, swimming, None);
    }

    /// Keeps the old projected witness out of the production frame path, including emotes.
    #[cfg(test)]
    fn capture_hud_player_with_emote_reference(
        &mut self,
        local: Option<client_world::ui_read::LocalPlayerUi<'_>>,
        swimming: bool,
        emote: Option<(cosmetics::emotes::Item, f64)>,
    ) {
        self.capture_hud_player_with_emote(local, swimming, emote);
        let live = &mut self.gui_models.live_player;
        live.body.reference(&mut live.vertices);
        live.cape.reference(&mut live.cape_vertices);
        discard_collapsed_faces(&mut live.cape_vertices);
        for (&item, model) in &live.hats {
            let vertices = live.hat_vertices.entry(item).or_default();
            model.reference(vertices);
            discard_collapsed_faces(vertices);
        }
    }
}

/// Converts a validated bone frame to block coordinates for equipment pose deltas.
fn bone_matrix(bone: &actor_animation::BoneTransform) -> Option<Affine3A> {
    let bone = render_bone(bone)?;
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
    if scale.abs().min_element() < f32::EPSILON {
        return None;
    }
    render_matrix(&bone)
}

/// Converts an evaluated model-space bone to the block coordinates shared by both renderers.
pub(super) fn render_bone(bone: &actor_animation::BoneTransform) -> Option<RenderBoneTransform> {
    RenderBoneTransform::from_model_space_scaled(
        bone.rotation,
        bone.translation_scale,
        bone.axis_scale,
    )
}

/// Builds a render-space frame, retaining zero scales that hide an authored bone.
pub(super) fn render_matrix(bone: &RenderBoneTransform) -> Option<Affine3A> {
    let rotation = Vec4::from_array(bone.rotation)
        .try_normalize()
        .map(Quat::from_vec4)?;
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
    Some(Affine3A::from_scale_rotation_translation(
        scale,
        rotation,
        Vec3::new(
            bone.translation_scale[0],
            bone.translation_scale[1],
            bone.translation_scale[2],
        ),
    ))
}

/// Removes cape faces with no lighting normal while preserving any remaining visible faces.
#[cfg(test)]
fn discard_collapsed_faces(vertices: &mut Vec<ActorVertex>) {
    let mut visible = 0;
    for start in (0..vertices.len()).step_by(3) {
        let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from_array(vertices[start + corner].position));
        if (b - a).cross(c - a).length_squared() == 0.0 {
            continue;
        }
        vertices.copy_within(start..start + 3, visible);
        visible += 3;
    }
    vertices.truncate(visible);
}

#[cfg(test)]
mod cape_tests;
#[cfg(test)]
mod hud_doll_tests;
#[cfg(test)]
mod redraw_tests;
#[cfg(test)]
pub(super) mod tests;
#[cfg(test)]
mod ui_context_tests;
