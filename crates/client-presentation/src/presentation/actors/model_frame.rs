//! Where a rig's model sits relative to its actor. Each actor class has one model frame and
//! sampled placements carry it, so early admission, culling, effect locators and light all
//! read the same model-to-world transform and actor position.

use assets::SkinGeometryBounds;
use client_world::ActorSnapshot;
use glam::Vec3;

use super::thrown_trident;

/// How an actor class places its model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelFrame {
    /// Stands on the actor's feet turned by body yaw; authored visible bounds stay upright.
    #[default]
    Upright,
    /// Turns freely about the actor's position by its flight rotation, as vanilla's native
    /// thrown-trident renderer places it.
    ThrownTrident,
}

impl ModelFrame {
    /// The frame `actor`'s class places its model in.
    pub fn of(actor: &ActorSnapshot) -> Self {
        if thrown_trident::is_thrown_trident(actor) {
            Self::ThrownTrident
        } else {
            Self::Upright
        }
    }

    /// World-from-model rows at the interpolated `position`, body `yaw` and `scale`, before
    /// authored axis scales and tilts. `axis_scale_y` is the authored Y axis scale those later
    /// apply, which a frame turning about a raised pivot must offset its origin by.
    pub(super) fn world_from_model(
        self,
        actor: &ActorSnapshot,
        alpha: f32,
        position: [f32; 3],
        yaw: f32,
        scale: f32,
        axis_scale_y: f32,
    ) -> [[f32; 4]; 3] {
        match self {
            Self::Upright => super::rig_world_from_actor(position, yaw, scale),
            Self::ThrownTrident => {
                thrown_trident::world_from_actor(actor, alpha, position, scale, axis_scale_y)
            }
        }
    }

    /// The actor's position in model space: the point a placed model maps onto it.
    fn model_anchor(self) -> Vec3 {
        match self {
            Self::Upright => Vec3::ZERO,
            Self::ThrownTrident => thrown_trident::native_pivot(),
        }
    }

    /// The actor position that `world_from_model`, as this frame placed it, maps the model
    /// anchor to.
    pub fn actor_position(self, world_from_model: &[[f32; 4]; 3]) -> [f32; 3] {
        let anchor = self.model_anchor();
        world_from_model
            .map(|row| row[0] * anchor.x + row[1] * anchor.y + row[2] * anchor.z + row[3])
    }

    /// Authored model `bounds` as the upright culling box placed at the model origin and
    /// grown by the model's largest axis scale: unchanged for an upright model, and a cube
    /// holding them in any rotation for a freely turning one.
    pub fn culling_bounds(self, bounds: SkinGeometryBounds) -> SkinGeometryBounds {
        match self {
            Self::Upright => bounds,
            Self::ThrownTrident => thrown_trident::rotation_invariant(bounds),
        }
    }
}

/// A sampled rig placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RigPlacement {
    /// The model-to-world rows the renderer draws with.
    pub world_from_model: [[f32; 4]; 3],
    /// The actor's interpolated world position, which anchors unlocated effects and light.
    pub position: [f32; 3],
    /// Body yaw used by the world transform, before axis scaling and tilts.
    pub yaw: f32,
    pub frame: ModelFrame,
}

impl RigPlacement {
    /// The model origin, where culling places the frame's culling bounds.
    pub fn model_origin(&self) -> [f32; 3] {
        self.world_from_model.map(|row| row[3])
    }
}
