//! Keeps the published first-person frame while republishing it would draw the same.
use super::hand::HandSource;
use render_model::{ActorRigSubmission, EquipmentRaster, HandItemAtlas, HandRigLight};

/// What the hand scene's frame was built from, kept only while rebuilding it could not change
/// what it draws: every draw's previous and current poses are equal, so the frame fraction each
/// instance carries blends nothing, and the draws, skin, light, projection, artwork and
/// geometry catalog are the same. Glint scrolling changes the draws themselves.
#[derive(Default)]
pub(super) struct PublishedHand {
    /// The frame-level inputs of the last published or kept frame, for a source taken unchanged
    /// across frames.
    quick: Option<QuickKey>,
    /// The recorded inputs and the scene's publication token for their frame.
    frame: Option<(Inputs, u64)>,
    /// Republications skipped through the quick check since the state was created.
    #[cfg(test)]
    pub(super) quick_kept: u64,
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

/// What a publication reads besides the source: its motion, light, projection, geometry catalog
/// and glint settings.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct QuickKey {
    motion: Option<[u32; 16]>,
    light: HandRigLight,
    fov: Option<u32>,
    geometry_revision: u64,
    glint: Option<(u32, u32)>,
}

impl QuickKey {
    pub(super) fn new(
        motion: Option<bevy::math::Mat4>,
        light: HandRigLight,
        fov: Option<f32>,
        geometry_revision: u64,
        glint: Option<&render_scene::UiGlintSettings>,
    ) -> Self {
        Self {
            motion: motion.map(|motion| motion.to_cols_array().map(f32::to_bits)),
            light,
            fov: fov.map(f32::to_bits),
            geometry_revision,
            glint: glint.map(|glint| (glint.strength.to_bits(), glint.speed.to_bits())),
        }
    }
}

impl PublishedHand {
    /// Whether the scene still holds the kept frame of a source taken unchanged since it was
    /// published, and every other input matches. The kept frame is still and its draws carry no
    /// item glint, whose scroll reads the frame fraction.
    pub(super) fn holds_quick(&self, quick: &QuickKey, held: Option<u64>) -> bool {
        self.quick == Some(*quick)
            && self.frame.as_ref().is_some_and(|(frame, publication)| {
                held == Some(*publication)
                    && frame.submissions.iter().all(|submission| {
                        submission.material.glint.texture != render_model::GlintTexture::Item
                    })
            })
    }

    /// Records the frame-level inputs of the frame just published or kept.
    pub(super) fn record_quick(&mut self, quick: QuickKey) {
        self.quick = Some(quick);
    }

    /// Whether the scene, holding the frame with publication token `held`, still holds the
    /// recorded frame and that frame was built from `inputs`. The scene assigns every accepted
    /// frame its own token, so a scene cleared or republished by anyone else since never matches.
    pub(super) fn holds(&mut self, inputs: &Inputs, held: Option<u64>) -> bool {
        let held = self
            .frame
            .as_ref()
            .is_some_and(|(frame, publication)| held == Some(*publication) && inputs.same(frame));
        #[cfg(test)]
        {
            self.kept += u64::from(held);
        }
        held
    }

    /// Records the inputs of the frame the scene just accepted with publication token
    /// `publication`, or forgets them.
    pub(super) fn record(&mut self, inputs: Option<Inputs>, publication: Option<u64>) {
        self.quick = None;
        self.frame = inputs.filter(|inputs| inputs.still).zip(publication);
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
