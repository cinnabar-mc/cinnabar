//! Keeps the published first-person frame while republishing it would draw the same.
use super::hand::HandSource;
use render_model::{ActorRigSubmission, EquipmentRaster, HandItemAtlas, HandRigLight};

/// What the hand scene's frame was built from, kept only while rebuilding it could not change
/// what it draws: every draw's previous and current poses are equal, so the frame fraction each
/// instance carries blends nothing, and the draws, skin, light, projection, artwork and
/// geometry catalog are the same. Glint scrolling changes the draws themselves.
#[derive(Default)]
pub(super) struct PublishedHand {
    /// The recorded inputs and the scene revision they were published under.
    frame: Option<(Inputs, u64)>,
    /// Republications skipped since the state was created.
    #[cfg(test)]
    pub(super) kept: u64,
}

/// One built frame's inputs; artwork compares by storage, so equal copies still republish.
pub(super) struct Inputs {
    /// Whether no draw's pose blends with the frame fraction.
    still: bool,
    submissions: Vec<ActorRigSubmission>,
    skin: render_api::SkinRgba8,
    light: HandRigLight,
    fov: u32,
    atlases: [Option<HandItemAtlas>; 3],
    glint: Option<EquipmentRaster>,
    geometry_revision: u64,
}

impl Inputs {
    /// Captures what a frame is built from.
    pub(super) fn new(
        submissions: Vec<ActorRigSubmission>,
        (source, skin): (&HandSource, render_api::SkinRgba8),
        (light, fov, geometry_revision): (HandRigLight, f32, u64),
        [main, off]: [Option<HandItemAtlas>; 2],
    ) -> Self {
        Self {
            still: submissions.iter().all(|submission| {
                std::sync::Arc::ptr_eq(
                    &submission.input.previous_bones,
                    &submission.input.current_bones,
                ) || submission.input.previous_bones == submission.input.current_bones
            }),
            submissions,
            skin,
            light,
            fov: fov.to_bits(),
            atlases: [main, off, source.maps.background.clone()],
            glint: source.item_glint.clone(),
            geometry_revision,
        }
    }

    /// The draws the frame is built from.
    pub(super) fn submissions(&self) -> &[ActorRigSubmission] {
        &self.submissions
    }

    /// Whether both frames would draw the same.
    fn same(&self, other: &Self) -> bool {
        let atlas =
            |left: &Option<HandItemAtlas>, right: &Option<HandItemAtlas>| match (left, right) {
                (None, None) => true,
                (Some(left), Some(right)) => {
                    (left.width, left.height, left.layers)
                        == (right.width, right.height, right.layers)
                        && left.rgba8.ptr_eq(&right.rgba8)
                }
                _ => false,
            };
        let glint = match (&self.glint, &other.glint) {
            (None, None) => true,
            (Some(left), Some(right)) => {
                (left.width, left.height) == (right.width, right.height)
                    && std::sync::Arc::ptr_eq(&left.rgba8, &right.rgba8)
            }
            _ => false,
        };
        self.still
            && other.still
            && self.submissions == other.submissions
            && self.skin == other.skin
            && self.light == other.light
            && self.fov == other.fov
            && self.geometry_revision == other.geometry_revision
            && glint
            && self
                .atlases
                .iter()
                .zip(&other.atlases)
                .all(|(left, right)| atlas(left, right))
    }
}

impl PublishedHand {
    /// Whether the scene, whose frame carries `held_revision`, still holds the recorded frame and
    /// that frame was built from `inputs`. A scene cleared or republished by anyone else since
    /// carries no revision or another one.
    pub(super) fn holds(&mut self, inputs: &Inputs, held_revision: Option<u64>) -> bool {
        let held = self.frame.as_ref().is_some_and(|(frame, revision)| {
            held_revision == Some(*revision) && inputs.same(frame)
        });
        #[cfg(test)]
        {
            self.kept += u64::from(held);
        }
        held
    }

    /// Records the inputs of the frame just published under `revision`, or forgets them.
    pub(super) fn record(&mut self, inputs: Option<Inputs>, revision: u64) {
        self.frame = inputs
            .filter(|inputs| inputs.still)
            .map(|inputs| (inputs, revision));
    }
}

/// Whether two frames of the same still draws place every vertex alike: they differ only in
/// generation and in the frame fraction that blends equal poses.
#[cfg(any(test, debug_assertions))]
pub(super) fn same_frame(
    held: &render_model::ActorRigRenderFrame,
    fresh: &render_model::ActorRigRenderFrame,
) -> bool {
    let instances = |frame: &render_model::ActorRigRenderFrame| {
        frame
            .instances
            .iter()
            .map(|instance| render_model::ActorGpuInstance {
                partial_tick: 0.0,
                ..*instance
            })
            .collect::<Vec<_>>()
    };
    instances(held) == instances(fresh)
        && held.previous_bones == fresh.previous_bones
        && held.current_bones == fresh.current_bones
        && held.previous_bones == held.current_bones
        && held.geometry_revision == fresh.geometry_revision
        && held.geometry_spans == fresh.geometry_spans
        && held.manifest == fresh.manifest
        && held.rasters == fresh.rasters
        && held.maximum_vertex_count == fresh.maximum_vertex_count
}
