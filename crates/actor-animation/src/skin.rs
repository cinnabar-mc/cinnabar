//! Player skins that carry their own model: the player's animations drive the skin's bones by
//! name, so the rig poses the skin geometry instead of the default humanoid.
use render_api::SkinGeometrySource;

use super::{pose::LocalDelta, *};

mod preparation;
use preparation::PreparedSkin;
pub(super) mod queue;
pub(super) use queue::SkinPreparationQueue;

#[derive(Debug)]
pub(super) enum SkinModel {
    Parsed(SkinSkeleton),
    /// The default geometry applies to this source (none named, or it failed to resolve).
    Default(Arc<SkinGeometrySource>),
}

#[derive(Debug)]
pub(super) struct SkinSkeleton {
    source: Arc<SkinGeometrySource>,
    pub(super) prepared: Arc<PreparedSkin>,
    /// Animation slot each skin bone takes: the default-rig bone of the same name, or the
    /// skin-only slot appended after the rig in `animated`.
    driver: Vec<Option<usize>>,
    /// The default rig followed by the skin's own bones, when the skin adds any.
    animated: Option<AnimatedSkeleton>,
    /// Geometry binding `driver` was matched against.
    driver_binding: usize,
}

/// The skeleton a skin-modelled player's clips are sampled on. Vanilla animates the skin
/// geometry itself, so channels naming bones only the skin defines must still pose them.
/// The default rig keeps its indices; skin-only bones follow it, parented within this array.
#[derive(Debug)]
pub(super) struct AnimatedSkeleton {
    bones: Vec<RuntimeBone>,
    names: Vec<Box<str>>,
    channels: Arc<pose::ChannelBindings>,
}

impl SkinModel {
    fn source(&self) -> &Arc<SkinGeometrySource> {
        match self {
            Self::Parsed(skeleton) => &skeleton.source,
            Self::Default(source) => source,
        }
    }
}

impl ActorRigState {
    pub(super) fn skin_skeleton(&self) -> Option<&SkinSkeleton> {
        match &self.skin {
            Some(SkinModel::Parsed(skeleton)) => Some(skeleton),
            _ => None,
        }
    }

    /// Bones the rig poses: the skin's when it has a model.
    pub(super) fn posed_bones(&self) -> &[RuntimeBone] {
        self.skin_skeleton()
            .map_or(&self.bones, |skeleton| &skeleton.prepared.bones)
    }

    /// Names of the bones the rig poses: the skin's when it has a model.
    pub(super) fn posed_bone_names(&self) -> &[Box<str>] {
        self.skin_skeleton()
            .map_or(&self.bone_names, |skeleton| &skeleton.prepared.names)
    }

    /// Bones, names and channel bindings clips are sampled on: the default rig, extended by any
    /// bones only the worn skin defines. Indices below the rig's bone count are the rig's.
    pub(super) fn animated_skeleton(
        &self,
    ) -> (&[RuntimeBone], &[Box<str>], &pose::ChannelBindings) {
        match self
            .skin_skeleton()
            .and_then(|skeleton| skeleton.animated.as_ref())
        {
            Some(animated) => (&animated.bones, &animated.names, &animated.channels),
            None => (&self.bones, &self.bone_names, &self.channel_bindings),
        }
    }

    /// Writes the selected skeleton into caller-owned storage without composition allocations.
    pub(super) fn compose_into(
        &self,
        local: &[LocalDelta],
        output: &mut [BoneTransform],
    ) -> Option<()> {
        match self.skin_skeleton() {
            Some(skeleton) => {
                let prepared = &skeleton.prepared;
                let program = prepared.composition.as_ref()?;
                program.compose_into(
                    &prepared.bones,
                    |index| {
                        skeleton.driver[index]
                            .and_then(|driver| local.get(driver).copied())
                            .unwrap_or_default()
                    },
                    output,
                )
            }
            None => self.composition.compose_into(
                &self.bones,
                |index| local.get(index).copied().unwrap_or_default(),
                output,
            ),
        }
    }

    /// Rematches the skin's bones to the default rig after the rig's geometry changed.
    pub(super) fn refresh_skin_drivers(&mut self) {
        let (bone_names, binding) = (&self.bone_names, self.geometry_binding);
        if let Some(SkinModel::Parsed(skeleton)) = &mut self.skin
            && skeleton.driver_binding != binding
        {
            (skeleton.driver, skeleton.animated) =
                drivers(&skeleton.prepared, &self.bones, bone_names);
            skeleton.driver_binding = binding;
            self.rest_on_posed_skeleton();
        }
    }

    /// Resets the stored poses to the posed skeleton's rest, after its bones changed.
    pub(super) fn rest_on_posed_skeleton(&mut self) {
        let rest = match self.skin_skeleton() {
            Some(skeleton) => skeleton.prepared.rest.as_ref().map(|rest| rest.to_vec()),
            None => compose_pose(&self.bones, &[]),
        };
        if let Some(rest) = rest {
            self.reset_to_skeleton(rest);
        }
    }

    /// Every pose a changed skeleton invalidates, including the HUD's, restarts from its rest.
    pub(super) fn reset_to_skeleton(&mut self, rest: Vec<BoneTransform>) {
        self.ui_pose = None;
        self.ui_animation = None;
        self.paper_doll_animation = None;
        self.previous.clone_from(&rest);
        self.current.clone_from(&rest);
        self.rest = rest;
        self.reset_pending = true;
        self.rest_reset_pending = true;
    }
}

/// How far skin bone `index` and its added descendants move so their offsets from a rig
/// parent match the skin's: the rig bone's pivot minus the skin bone's for a matched bone,
/// its parent's shift for an added bone, and none at a root. Results are memoized in `shift`.
fn shift_of(
    index: usize,
    skin: &PreparedSkin,
    rig: &[RuntimeBone],
    driver: &[Option<usize>],
    shift: &mut [Option<[f32; 3]>],
) -> [f32; 3] {
    if let Some(known) = shift[index] {
        return known;
    }
    // Mark the bone before following its parent, so a malformed cycle ends at no shift.
    shift[index] = Some([0.0; 3]);
    let value = match driver[index] {
        Some(slot) if slot < rig.len() => {
            std::array::from_fn(|axis| rig[slot].pivot[axis] - skin.bones[index].pivot[axis])
        }
        _ => match skin.bones[index].parent {
            Some(parent) if parent < skin.bones.len() => shift_of(parent, skin, rig, driver, shift),
            _ => [0.0; 3],
        },
    };
    shift[index] = Some(value);
    value
}

/// Matches each skin bone to the rig bone of the same name, appending a skin-only animation
/// slot for each bone the rig lacks. Returns no extended skeleton when every bone matched.
fn drivers(
    skin: &PreparedSkin,
    rig: &[RuntimeBone],
    rig_names: &[Box<str>],
) -> (Vec<Option<usize>>, Option<AnimatedSkeleton>) {
    let mut driver: Vec<Option<usize>> = skin
        .names
        .iter()
        .map(|name| rig_names.iter().position(|candidate| candidate == name))
        .collect();
    let mut next = rig.len();
    let mut own = Vec::new();
    for (index, slot) in driver.iter_mut().enumerate() {
        if slot.is_none() && skin.bones.get(index).is_some() {
            *slot = Some(next);
            own.push(index);
            next += 1;
        }
    }
    if own.is_empty() {
        return (driver, None);
    }
    // An added bone keeps its skin-authored offset from its parent: channel defaults read that
    // offset, so its pivot moves with the rig parent that now stands in for the skin parent.
    let mut shift: Vec<Option<[f32; 3]>> = vec![None; skin.bones.len()];
    let mut bones = rig.to_vec();
    let mut names = rig_names.to_vec();
    for &index in &own {
        let mut bone = skin.bones[index].clone();
        let offset = shift_of(index, skin, rig, &driver, &mut shift);
        bone.pivot = std::array::from_fn(|axis| bone.pivot[axis] + offset[axis]);
        bone.parent = bone
            .parent
            .and_then(|parent| driver.get(parent).copied().flatten());
        bones.push(bone);
        names.push(skin.names[index].clone());
    }
    let channels = Arc::new(pose::ChannelBindings::new(&names));
    (
        driver,
        Some(AnimatedSkeleton {
            bones,
            names,
            channels,
        }),
    )
}

/// Brings the rig's skin model in line with the skin the actor now wears. Returns whether a
/// source was rejected.
pub(super) fn sync_skin(
    state: &mut ActorRigState,
    source: Option<&Arc<SkinGeometrySource>>,
    cache: &SkinPreparationQueue,
) -> bool {
    let unchanged = match (&state.skin, source) {
        (None, None) => true,
        (Some(model), Some(source)) => Arc::ptr_eq(model.source(), source),
        _ => false,
    };
    if unchanged {
        return false;
    }
    let outcome = match source {
        Some(source) => {
            let Some(outcome) = cache.get(source) else {
                return false;
            };
            Some(outcome)
        }
        None => None,
    };
    if let (Some(model), Some(source)) = (&mut state.skin, source) {
        let shares_preparation = match (model, outcome.as_ref()) {
            (SkinModel::Parsed(skeleton), Some((Some(prepared), _))) => {
                Arc::ptr_eq(&skeleton.prepared, prepared)
            }
            _ => false,
        };
        let model = state
            .skin
            .as_mut()
            .expect("the retained skin was inspected");
        if shares_preparation || cache.replaces_unchanged(source, model.source()) {
            match model {
                SkinModel::Parsed(skeleton) => skeleton.source = Arc::clone(source),
                SkinModel::Default(previous) => *previous = Arc::clone(source),
            }
            return false;
        }
    }
    let had_skeleton = state.skin_skeleton().is_some();
    let mut rejected = false;
    state.skin = source.map(|source| {
        let (prepared, invalid) = outcome
            .clone()
            .expect("a source requires completed preparation");
        rejected = invalid;
        match prepared {
            Some(prepared) => {
                let (driver, animated) = drivers(&prepared, &state.bones, &state.bone_names);
                SkinModel::Parsed(SkinSkeleton {
                    source: Arc::clone(source),
                    driver,
                    animated,
                    driver_binding: state.geometry_binding,
                    prepared,
                })
            }
            None => SkinModel::Default(Arc::clone(source)),
        }
    });
    if had_skeleton || state.skin_skeleton().is_some() {
        state.rest_on_posed_skeleton();
    }
    rejected
}

/// Skin preparations the calling thread has run.
#[cfg(any(test, feature = "test-support"))]
pub(super) fn prepared_on_this_thread() -> usize {
    preparation::PREPARED_ON_THREAD.with(std::cell::Cell::get)
}

#[cfg(test)]
#[path = "skin/tests.rs"]
mod tests;
