//! Visible triangle masks with bounded, owner-scoped nesting.

use super::*;

#[cfg(test)]
mod tests;

/// Mask placement and optional intersection with a parent mask.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub geometry: Handle,
    pub anchor: Anchor,
    pub transform: Transform,
    pub parent: Option<Handle>,
}

impl Clip {
    /// Resolves mask placement through the same anchor path used by ordinary geometry.
    pub fn instance(&self) -> Instance {
        Instance {
            geometry: self.geometry,
            material: None,
            clip: None,
            params: [0.; mod_api::materials::PARAMS],
            anchor: self.anchor,
            transform: self.transform,
            tint: [1.; 4],
            depth: Depth::Test,
            blend: Blend::Premultiplied,
            width: 1.,
            width_mode: WidthMode::World,
            join: Join::Bevel,
            cap: Cap::Butt,
        }
    }
}

impl Store {
    /// Retains a mask only when geometry, nesting, ownership and drawing grants are valid.
    pub fn create_clip(
        &mut self,
        clip: Clip,
        permissions: DrawingPermissions,
    ) -> Result<Handle, DrawError> {
        let handle = self.token()?;
        self.edit(size_of::<Clip>(), |resources| {
            validate(&clip, resources, permissions)?;
            if resources.clips.len() >= mod_api::drawing::MAX_DRAW_CLIPS {
                return Err(error(ErrorCode::BudgetExceeded, "clips"));
            }
            resources.clips.insert(handle, clip);
            resources.instance_revision += 1;
            Ok(())
        })?;
        Ok(handle)
    }

    /// Reports worker completion or its explicit failure without doing geometry work.
    pub fn geometry_status(&self, id: Handle) -> Result<bool, DrawError> {
        self.check(id)?;
        let resources = self.staged.as_ref().unwrap_or(&self.committed);
        let geometry = resources
            .geometry
            .get(&id)
            .ok_or(error(ErrorCode::StaleHandle, "geometry"))?;
        match geometry.blocks().and_then(|grid| grid.result()) {
            Some(Err(error)) => Err(error.clone()),
            Some(Ok(_)) => Ok(true),
            None => Ok(geometry.blocks().is_none()),
        }
    }
}

/// Validates mask placement and walks at most the supported number of parent clips.
pub(super) fn validate(
    clip: &Clip,
    resources: &Resources,
    permissions: DrawingPermissions,
) -> Result<(), DrawError> {
    validation::instance(&clip.instance(), resources, permissions)?;
    let geometry = &resources.geometry[&clip.geometry];
    if geometry.polyline || geometry.blocks.is_some() {
        return Err(error(ErrorCode::Invalid, "clip needs triangles"));
    }
    let mut parent = clip.parent;
    let mut depth = 1;
    while let Some(id) = parent {
        depth += 1;
        if depth > mod_api::drawing::MAX_DRAW_CLIP_DEPTH {
            return Err(error(ErrorCode::BudgetExceeded, "clip nesting"));
        }
        parent = resources
            .clips
            .get(&id)
            .ok_or(error(ErrorCode::StaleHandle, "parent clip"))?
            .parent;
    }
    Ok(())
}

/// Drops masks whose anchor or parent vanished, followed by their dependent placements.
pub(super) fn reconcile(resources: &mut Resources) {
    resources.clips.retain(|_,clip|!matches!(clip.anchor,Anchor::Entity(anchor) if !resources.entities.contains_key(&anchor.entity)));
    for _ in 0..mod_api::drawing::MAX_DRAW_CLIP_DEPTH {
        let live: std::collections::BTreeSet<_> = resources.clips.keys().copied().collect();
        resources
            .clips
            .retain(|_, clip| clip.parent.is_none_or(|id| live.contains(&id)));
    }
    resources.instances.retain(|_, instance| {
        instance
            .clip
            .is_none_or(|id| resources.clips.contains_key(&id))
    });
}
