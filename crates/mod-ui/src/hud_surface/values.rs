//! Binding values as JSON-UI scalars, and per-frame projection of world and actor points.

use json_ui::Scalar;
use serde_json::json;
use ui::mod_panel::{Projection, SurfaceValue};

/// A plain value's scalar; item and projected values are resolved separately.
pub(crate) fn scalar(value: &SurfaceValue) -> Scalar {
    match value {
        SurfaceValue::Bool(value) => Scalar::Bool(*value),
        SurfaceValue::Number(value) => Scalar::Num(*value),
        SurfaceValue::Text(value) => Scalar::Text(value.clone()),
        SurfaceValue::Vector(value) => Scalar::Json(json!(value)),
        SurfaceValue::Item(_) => Scalar::Json(serde_json::Value::Null),
        SurfaceValue::Project(_) => Scalar::Json(json!([0., 0.])),
    }
}

/// An actor a projection may follow, at its interpolated feet position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectedActor {
    pub feet: [f32; 3],
    /// Players need the players grant; every other actor needs entities.
    pub player: bool,
}

/// Which actors a mod's projections may follow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActorAuthority {
    pub players: bool,
    pub entities: bool,
}

impl ActorAuthority {
    /// Whether `actor` is observable under this authority.
    pub fn admits(self, actor: ProjectedActor) -> bool {
        if actor.player {
            self.players
        } else {
            self.entities
        }
    }
}

/// Where the root lies on the window, all in GUI units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RootPlacement {
    /// The root's offset from the window's top left.
    pub origin: [f64; 2],
    pub size: [f64; 2],
    pub window: [f64; 2],
}

/// A projected point's `[x, y]` GUI offset from the root's top left, or none when the actor
/// is absent or not admitted, or the point is behind the camera or outside the root.
pub(crate) fn project(
    projection: &Projection,
    clip_from_world: Option<&glam::Mat4>,
    root: RootPlacement,
    actor: &dyn Fn(u64) -> Option<ProjectedActor>,
    authority: ActorAuthority,
) -> Option<[f64; 2]> {
    let matrix = clip_from_world?;
    let world = match projection {
        Projection::World(point) => point.world.map(|axis| axis as f32),
        Projection::Actor(point) => {
            let actor = actor(point.actor).filter(|actor| authority.admits(*actor))?;
            std::array::from_fn(|axis| actor.feet[axis] + point.offset[axis] as f32)
        }
    };
    let clip = *matrix * glam::Vec3::from_array(world).extend(1.);
    if !clip.is_finite() || clip.w <= 0.001 || clip.z < 0. || clip.z > clip.w {
        return None;
    }
    let ndc = [f64::from(clip.x / clip.w), f64::from(clip.y / clip.w)];
    let offset = [
        (ndc[0] * 0.5 + 0.5) * root.window[0] - root.origin[0],
        (0.5 - ndc[1] * 0.5) * root.window[1] - root.origin[1],
    ];
    (offset
        .iter()
        .zip(root.size)
        .all(|(at, size)| at.is_finite() && (0. ..=size).contains(at)))
    .then_some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::mod_panel::{ActorPoint, WorldPoint};

    const ROOT: RootPlacement = RootPlacement {
        origin: [0.; 2],
        size: [640., 360.],
        window: [640., 360.],
    };

    fn camera() -> glam::Mat4 {
        glam::camera::rh::proj::directx::perspective(1.0, 16. / 9., 0.1, 1000.)
    }

    fn world(at: [f64; 3]) -> Projection {
        Projection::World(WorldPoint { world: at })
    }

    fn none(_: u64) -> Option<ProjectedActor> {
        None
    }

    #[test]
    fn points_ahead_project_to_root_offsets_and_others_hide() {
        let matrix = camera();
        let centre = project(
            &world([0., 0., -10.]),
            Some(&matrix),
            ROOT,
            &none,
            Default::default(),
        );
        assert_eq!(centre, Some([320., 180.]));
        for hidden in [[0., 0., 10.], [100., 0., -10.], [0., 0., -2000.]] {
            assert!(
                project(
                    &world(hidden),
                    Some(&matrix),
                    ROOT,
                    &none,
                    Default::default()
                )
                .is_none(),
                "{hidden:?}"
            );
        }
        assert!(
            project(
                &world([0., 0., -10.]),
                None,
                ROOT,
                &none,
                Default::default()
            )
            .is_none()
        );
        let inset = RootPlacement {
            origin: [20., 10.],
            size: [600., 340.],
            window: [640., 360.],
        };
        assert_eq!(
            project(
                &world([0., 0., -10.]),
                Some(&matrix),
                inset,
                &none,
                Default::default()
            ),
            Some([300., 170.])
        );
    }

    #[test]
    fn actor_points_need_the_actor_and_its_grant() {
        let matrix = camera();
        let follow = Projection::Actor(ActorPoint {
            actor: 5,
            offset: [0., 2., 0.],
        });
        let actor = |id: u64| {
            (id == 5).then_some(ProjectedActor {
                feet: [0., -2., -10.],
                player: true,
            })
        };
        let players = ActorAuthority {
            players: true,
            entities: false,
        };
        assert_eq!(
            project(&follow, Some(&matrix), ROOT, &actor, players),
            Some([320., 180.])
        );
        let entities = ActorAuthority {
            players: false,
            entities: true,
        };
        assert!(project(&follow, Some(&matrix), ROOT, &actor, entities).is_none());
        assert!(project(&follow, Some(&matrix), ROOT, &none, players).is_none());
    }
}
