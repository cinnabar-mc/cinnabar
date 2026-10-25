//! Thrown tridents, which vanilla draws with a native renderer: the model turns about the
//! actor's own position by its flight rotation instead of standing upright on its feet.

use assets::{SkinGeometryBounds, facts::entity::NativeEntityClass};
use client_world::ActorSnapshot;
use glam::{Mat3, Vec3};

use super::{DEGREES_TO_RADIANS, lerp_degrees};

/// The native model's origin sits this many authored pixels above the geometry origin,
/// because native model parts load authored heights as 24 minus Y.
const NATIVE_ORIGIN_PIXELS: f32 = 24.0;
/// Model pixels per block.
const PIXELS_PER_BLOCK: f32 = 16.0;
/// The native renderer's offsets to the actor's yaw and pitch, in degrees.
const YAW_OFFSET_DEGREES: f32 = -90.0;
const PITCH_OFFSET_DEGREES: f32 = 90.0;
/// Vanilla's glint UV scale for the thrown trident's foil pass.
const FOIL_UV_SCALE: f32 = 0.5;

fn is_thrown_trident(actor: &ActorSnapshot) -> bool {
    actor.entity_fact.native_class == NativeEntityClass::ThrownTrident
}

/// The thrown trident's world-from-rig rows at its interpolated `position`, rotation and
/// `scale`; `None` for every other actor class.
pub(super) fn thrown_trident_world_from_actor(
    actor: &ActorSnapshot,
    alpha: f32,
    position: [f32; 3],
    scale: f32,
) -> Option<[[f32; 4]; 3]> {
    is_thrown_trident(actor).then(|| {
        world_from_rig(
            position,
            lerp_degrees(actor.previous_pose.yaw, actor.yaw, alpha),
            lerp_degrees(actor.previous_pose.pitch, actor.pitch, alpha),
            scale,
        )
    })
}

/// The world position a placed thrown trident turns about, which is the actor's own position
/// rather than the rig origin trailing behind it; `None` for every other actor class.
pub(super) fn thrown_trident_position(
    actor: &ActorSnapshot,
    world_from_rig: &[[f32; 4]; 3],
) -> Option<[f32; 3]> {
    let height = NATIVE_ORIGIN_PIXELS / PIXELS_PER_BLOCK;
    is_thrown_trident(actor).then(|| world_from_rig.map(|row| row[1] * height + row[3]))
}

/// Places the rig frame (authored X mirrored, Y up, in blocks) as the native renderer places
/// its Y-down model: at the actor's position, turned by `yaw - 90` about Y and then by
/// `pitch + 90` about Z. The native model frame is the rig frame turned half a turn about Z
/// and lifted to the 24-pixel origin.
fn world_from_rig(position: [f32; 3], yaw: f32, pitch: f32, scale: f32) -> [[f32; 4]; 3] {
    let turn = Mat3::from_rotation_y((yaw + YAW_OFFSET_DEGREES) * DEGREES_TO_RADIANS)
        * Mat3::from_rotation_z((pitch + PITCH_OFFSET_DEGREES) * DEGREES_TO_RADIANS);
    let linear = turn * Mat3::from_rotation_z(std::f32::consts::PI) * scale;
    let origin = Vec3::from_array(position)
        + turn * Vec3::Y * (NATIVE_ORIGIN_PIXELS / PIXELS_PER_BLOCK * scale);
    let row = |axis: usize| {
        [
            linear.x_axis[axis],
            linear.y_axis[axis],
            linear.z_axis[axis],
            origin[axis],
        ]
    };
    [row(0), row(1), row(2)]
}

/// Gives a placed thrown trident culling bounds that hold in any flight rotation and, when the
/// server marks it enchanted, the native foil pass over its own texture.
pub(super) fn present_thrown_trident(
    submission: &mut render_model::ActorRigSubmission,
    actor: &ActorSnapshot,
) {
    if !is_thrown_trident(actor) {
        return;
    }
    submission.culling_bounds = rotation_invariant(submission.culling_bounds);
    if actor.is_enchanted() {
        submission.material.glint = render_model::ActorGlint {
            profile: render_api::CombatVisuals::Modern,
            uv_scale: [FOIL_UV_SCALE; 2],
            texture: render_model::GlintTexture::Item,
            ..Default::default()
        };
    }
}

/// A cube about the rig origin containing `bounds` under every rotation.
fn rotation_invariant(bounds: SkinGeometryBounds) -> SkinGeometryBounds {
    let radius =
        Vec3::from_array(bounds.center).length() + Vec3::from_array(bounds.half_extents).length();
    SkinGeometryBounds {
        center: [0.0; 3],
        half_extents: [radius; 3],
    }
}

#[cfg(test)]
#[path = "thrown_trident_tests.rs"]
mod tests;
