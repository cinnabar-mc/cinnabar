//! The head part's own rotation, for the native layer that reads it instead of the composed pose.
use super::*;

/// The bone the native snow golem layer finds by name.
const HEAD_PART: &str = "head";

/// The head part's own X and Y rotation at the previous and current endpoint, in authored degrees.
///
/// These are the evaluated channel values before composition, so neither a parent's rotation nor
/// the part's Z rotation is in them. A composed quaternion cannot give them back: `[20, 90, 30]`
/// and `[-10, 90, 0]` compose to the same rotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadRotation {
    pub previous: [f32; 2],
    pub current: [f32; 2],
}

impl HeadRotation {
    /// Both endpoints at one pose, for a rig with no earlier pose to interpolate from.
    const fn at(angles: [f32; 2]) -> Self {
        Self {
            previous: angles,
            current: angles,
        }
    }

    /// The rest rotation of a rig whose actor needs its head's angles. Every other rig keeps none,
    /// which is all its evaluation pays for this. A rig without a head part keeps none either.
    pub(super) fn rest(
        actor: &dyn AnimatedActor,
        bones: &[RuntimeBone],
        names: &[Box<str>],
    ) -> Option<Self> {
        let reads_head = matches!(actor.kind(), ActorKind::Entity { identifier }
            if identifier.as_ref() == assets::facts::entity::SNOW_GOLEM_IDENTIFIER);
        reads_head
            .then(|| angles(bones, names, &[]).map(Self::at))
            .flatten()
    }

    /// The endpoints after a tick, following the body's pose history: a restart starts from `next`,
    /// and only an advancing tick retires the current endpoint.
    pub(super) fn carry(
        old: Option<Self>,
        next: Option<[f32; 2]>,
        restart: bool,
        advance_history: bool,
    ) -> Option<Self> {
        let current = next?;
        Some(match old.filter(|_| !restart) {
            Some(old) => Self {
                previous: if advance_history {
                    old.current
                } else {
                    old.previous
                },
                current,
            },
            None => Self::at(current),
        })
    }

    /// Endpoints of a frame sample. A sample with no previous endpoint draws one pose, as the body
    /// does when it has no sampled previous pose.
    pub(super) fn sampled(
        bones: &[RuntimeBone],
        names: &[Box<str>],
        current: &[pose::LocalDelta],
        previous: Option<&[pose::LocalDelta]>,
    ) -> Option<Self> {
        let current = angles(bones, names, current)?;
        Some(Self {
            previous: match previous {
                Some(previous) => angles(bones, names, previous)?,
                None => current,
            },
            current,
        })
    }
}

/// The head part's own `[x, y]` rotation in one set of local deltas, in authored degrees. A missing
/// delta reads as the bone's rest rotation, as it does in composition.
pub(super) fn angles(
    bones: &[RuntimeBone],
    names: &[Box<str>],
    local: &[pose::LocalDelta],
) -> Option<[f32; 2]> {
    let index = names.iter().position(|name| name.as_ref() == HEAD_PART)?;
    let rest = bones.get(index)?.rotation;
    let delta = local.get(index).copied().unwrap_or_default().rotation;
    Some([rest[0] + delta[0], rest[1] + delta[1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_drops_the_old_endpoints_and_only_an_advance_retires_current() {
        let old = HeadRotation {
            previous: [1.0, 2.0],
            current: [3.0, 4.0],
        };
        let next = Some([5.0, 6.0]);
        let carry = |restart, advance| HeadRotation::carry(Some(old), next, restart, advance);
        assert_eq!(
            carry(false, true),
            Some(HeadRotation {
                previous: [3.0, 4.0],
                current: [5.0, 6.0],
            })
        );
        assert_eq!(
            carry(false, false),
            Some(HeadRotation {
                previous: [1.0, 2.0],
                current: [5.0, 6.0],
            })
        );
        assert_eq!(carry(true, true), Some(HeadRotation::at([5.0, 6.0])));
        assert_eq!(
            HeadRotation::carry(None, next, false, true),
            Some(HeadRotation::at([5.0, 6.0]))
        );
        assert_eq!(HeadRotation::carry(Some(old), None, false, true), None);
    }

    #[test]
    fn angles_add_the_bone_s_rest_rotation_to_its_own_delta_and_skip_other_bones() {
        let bone = |rotation| RuntimeBone {
            rotation,
            ..Default::default()
        };
        let bones = [bone([9.0, 9.0, 9.0]), bone([1.0, 2.0, 3.0])];
        let names: [Box<str>; 2] = ["body".into(), "head".into()];
        let delta = |rotation| pose::LocalDelta {
            rotation,
            ..Default::default()
        };
        let local = [delta([50.0, 50.0, 50.0]), delta([10.0, 20.0, 30.0])];
        assert_eq!(angles(&bones, &names, &local), Some([11.0, 22.0]));
        assert_eq!(angles(&bones, &names, &[]), Some([1.0, 2.0]));
        assert_eq!(angles(&bones[..1], &names[..1], &local), None);
    }
}
