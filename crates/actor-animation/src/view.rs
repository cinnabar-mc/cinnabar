use assets::SkinGeometryBounds;

impl super::ActorRigSnapshot<'_> {
    /// Uses the displayed model's authored visibility box independently of collision metadata.
    #[must_use]
    pub fn culling_bounds(&self) -> SkinGeometryBounds {
        if let Some(geometry) = self.skin_geometry {
            return geometry.visible_bounds.unwrap_or_default();
        }
        self.geometry_source()
            .and_then(|(assets, geometry)| assets.geometry_visible_bounds(geometry))
            .unwrap_or_default()
    }
}

/// Vanilla's render bounds for one actor: the model's authored visibility box joined with the
/// actor's own collision box, both relative to its feet. Vanilla clamps an authored box to 50
/// blocks, so a tall model whose box sits high above its actor still draws while the actor
/// itself is in view. An actor without collision dimensions keeps the authored box alone.
#[must_use]
pub fn actor_culling_bounds(
    authored: SkinGeometryBounds,
    actor: &dyn crate::AnimatedActor,
) -> SkinGeometryBounds {
    let Some((low, high)) = actor.bounding_box() else {
        return authored;
    };
    let feet = actor.position();
    let collision = SkinGeometryBounds::from_corners(
        std::array::from_fn(|axis| low[axis] - feet[axis]),
        std::array::from_fn(|axis| high[axis] - feet[axis]),
    );
    if (0..3).any(|axis| {
        !(collision.center[axis].is_finite() && collision.half_extents[axis].is_finite())
    }) {
        return authored;
    }
    authored.union(collision)
}

/// The view animation runs for: actors outside it hold their pose, since vanilla evaluates
/// pre-animation, animation and render controllers only for actors it renders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorAnimationView {
    /// Clip-space half-spaces `[a, b, c, d]`, inside where `a*x + b*y + c*z + d >= 0`.
    pub planes: [[f32; 4]; 6],
    pub camera: [f32; 3],
    /// Players farther than this from the camera hold their pose.
    pub player_distance: f32,
    /// Other actors farther than this from the camera on any axis hold their pose.
    pub entity_radius: f32,
}

impl ActorAnimationView {
    /// Keeps authored model bounds inside the guard-banded animation frustum.
    #[must_use]
    pub fn admits(
        &self,
        feet: [f32; 3],
        scale: f32,
        player: bool,
        bounds: SkinGeometryBounds,
    ) -> bool {
        if feet.iter().any(|value| !value.is_finite()) {
            return true;
        }
        let offset: [f32; 3] = std::array::from_fn(|axis| feet[axis] - self.camera[axis]);
        if player {
            let head = [offset[0], offset[1] + 1.0, offset[2]];
            if head.iter().map(|value| value * value).sum::<f32>()
                > self.player_distance * self.player_distance
            {
                return false;
            }
        } else if offset.iter().any(|value| value.abs() > self.entity_radius) {
            return false;
        }
        let (low, high) = bounds.at(feet, scale);
        // Keep the pose live for camera-containing boxes admitted by the render culler.
        if (0..3).all(|axis| low[axis] <= self.camera[axis] && self.camera[axis] <= high[axis]) {
            return true;
        }
        self.planes.iter().all(|plane| {
            let corner: [f32; 3] = std::array::from_fn(|axis| {
                if plane[axis] >= 0.0 {
                    high[axis]
                } else {
                    low[axis]
                }
            });
            plane[0] * corner[0] + plane[1] * corner[1] + plane[2] * corner[2] + plane[3] >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ActorAnimationView, actor_culling_bounds};
    use crate::test_actor::TestActor;

    /// The half-space `x <= 10` alone, with generous distances.
    fn wall() -> ActorAnimationView {
        ActorAnimationView {
            planes: [[-1.0, 0.0, 0.0, 10.0]; 6],
            camera: [0.0; 3],
            player_distance: 100.0,
            entity_radius: 72.0,
        }
    }

    #[test]
    fn a_box_straddling_a_plane_is_admitted_and_one_past_it_is_not() {
        assert!(wall().admits([10.4, 0.0, 0.0], 1.0, false, Default::default()));
        assert!(!wall().admits([10.6, 0.0, 0.0], 1.0, false, Default::default()));
        // A scaled box reaches farther across the plane.
        assert!(wall().admits([11.5, 0.0, 0.0], 4.0, false, Default::default()));
    }

    #[test]
    fn entities_use_the_candidate_cube_and_players_the_distance() {
        let view = ActorAnimationView {
            planes: [[0.0, 0.0, 0.0, 1.0]; 6],
            ..wall()
        };
        assert!(!view.admits([0.0, 0.0, 73.0], 1.0, false, Default::default()));
        assert!(view.admits([0.0, 0.0, 73.0], 1.0, true, Default::default()));
        assert!(!view.admits([0.0, 0.0, 101.0], 1.0, true, Default::default()));
    }

    #[test]
    fn authored_visibility_box_keeps_edge_skin_animation_running() {
        let bounds = assets::SkinGeometryBounds {
            center: [0.0, 2.0, 0.0],
            half_extents: [1.5, 2.0, 1.5],
        };
        assert!(!wall().admits([11.0, 0.0, 0.0], 1.0, true, Default::default()));
        assert!(wall().admits([11.0, 0.0, 0.0], 1.0, true, bounds));
        assert!(!wall().admits([11.6, 0.0, 0.0], 1.0, true, bounds));
        let above = ActorAnimationView {
            planes: [[0.0, 1.0, 0.0, -3.0]; 6],
            camera: [0.0, -1.0, 0.0],
            ..wall()
        };
        assert!(!above.admits([0.0; 3], 1.0, true, Default::default()));
        assert!(above.admits([0.0; 3], 1.0, true, bounds));
    }

    /// A model box clamped high above its actor, as a tall beam's is.
    fn raised_box() -> assets::SkinGeometryBounds {
        assets::SkinGeometryBounds {
            center: [0.0, 256.0, 0.0],
            half_extents: [2.0, 25.0, 2.0],
        }
    }

    #[test]
    fn the_collision_box_keeps_a_raised_model_box_admitted_while_its_actor_is_in_view() {
        // Only the space below y = 10 is in view; the actor stands inside it.
        let low = ActorAnimationView {
            planes: [[0.0, -1.0, 0.0, 10.0]; 6],
            camera: [0.0, -5.0, 0.0],
            ..wall()
        };
        let mut actor = TestActor {
            position: [3.0, 4.0, 5.0],
            ..TestActor::default()
        };
        assert!(!low.admits(actor.position, 1.0, false, raised_box()));
        actor.status.bounding_box = Some(([2.9975, 4.0, 4.9975], [3.0025, 4.005, 5.0025]));
        let joined = actor_culling_bounds(raised_box(), &actor);
        assert!(low.admits(actor.position, 1.0, false, joined));
        let (bottom, top) = joined.at(actor.position, 1.0);
        assert!((bottom[1] - 4.0).abs() < 1e-4 && (top[1] - 285.0).abs() < 1e-4);
    }

    #[test]
    fn an_actor_without_collision_keeps_its_authored_box() {
        let actor = TestActor::default();
        assert_eq!(actor_culling_bounds(raised_box(), &actor), raised_box());
    }
}
