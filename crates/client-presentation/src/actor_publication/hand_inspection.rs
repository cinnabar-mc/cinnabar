//! One-shot registration observations around the normal first-person build.
use render_model::{ActorArtworkLocation, ActorRigFrameBuilder, ActorRigSubmission};
use render_scene::{ActorRenderScene, HandRigScene};
use serde_json::{Value, json};

const MAX_SUBMISSIONS: usize = 64;

/// A state request arms one publication, not an extra render or animation sample.
#[derive(Default)]
pub(super) struct HandInspection {
    requested: bool,
    snapshot: Option<Value>,
}

impl HandInspection {
    /// Clears an old session's evidence without dropping a newly armed request.
    pub(super) fn clear_snapshot(&mut self) {
        self.snapshot = None;
    }
    /// Requests fresh observations at the next ordinary hand publication.
    pub(super) fn request(&mut self) {
        self.requested = true;
    }
    /// Consumes the request so ordinary frames do no diagnostic allocation or lookup.
    pub(super) fn take_request(&mut self) -> bool {
        std::mem::take(&mut self.requested)
    }
    /// Retains the observation independently of whether the final hand scene draws.
    pub(super) fn snapshot(&self) -> Option<&Value> {
        self.snapshot.as_ref()
    }

    /// Reads already-selected submissions, without admitting geometry or sampling controllers.
    pub(super) fn before(
        builder: &ActorRigFrameBuilder,
        world: &ActorRenderScene,
        pack: Option<&assets::RuntimeEntityAssets>,
        submissions: impl IntoIterator<
            Item = (
                &'static str,
                ActorRigSubmission,
                Option<ActorArtworkLocation>,
            ),
        >,
    ) -> Value {
        let mut total = 0;
        let mut rows = Vec::new();
        for (category, submission, location) in submissions {
            let index = total;
            total += 1;
            if rows.len() == MAX_SUBMISSIONS {
                continue;
            }
            let rig = submission.input.rig;
            let catalog = pack.and_then(|pack| {
                pack.geometries().iter().enumerate().find(|(index, _)| {
                    render_model::layer_geometry_rig_id(render_model::pack_rig_id(0), *index as u32)
                        == rig
                })
            });
            rows.push(json!({
                "submission_index": index, "category": category, "requested_rig": rig.0,
                "route": format!("{:?}", submission.route),
                "identity": format!("{:?}", submission.input.identity),
                "pack_geometry_index": catalog.map(|(index, _)| index),
                "pack_geometry_name": catalog.map(|(_, geometry)| geometry.identifier.as_ref()),
                "pack_geometry_source": catalog.and_then(|(_, geometry)| pack?.sources().get(geometry.source_index as usize)).map(|source| source.path.as_ref()),
                "hand_registered_before": builder.contains_geometry(rig),
                "world_registered_before": world.contains_geometry(rig),
                "hand_can_draw_before": builder.can_draw_submission(&submission),
                "previous_bones": submission.input.previous_bones.len(),
                "current_bones": submission.input.current_bones.len(),
                "world_from_actor": submission.world_from_actor,
                "artwork_location": location.map(|location| json!({"page":location.page,"layer":location.layer})),
            }));
        }
        json!({"total_submissions":total,"omitted_submissions":total-rows.len(),"submissions":rows})
    }

    /// Joins against the normal build's final manifest; readiness is not an admission verdict.
    pub(super) fn finish(
        &mut self,
        mut report: Value,
        builder: &ActorRigFrameBuilder,
        scene: &HandRigScene,
    ) {
        let frame = scene.frame.as_ref();
        let truncated = report["omitted_submissions"].as_u64().unwrap_or(0) != 0;
        if let Some(rows) = report["submissions"].as_array_mut() {
            let mut counts = std::collections::BTreeMap::new();
            for row in rows.iter() {
                *counts
                    .entry((
                        row["requested_rig"].as_u64(),
                        row["identity"].as_str().unwrap_or("").to_owned(),
                    ))
                    .or_insert(0) += 1;
            }
            for row in rows {
                let rig = render_model::EntityRigId(row["requested_rig"].as_u64().unwrap() as u32);
                row["hand_registered_after"] = json!(builder.contains_geometry(rig));
                let matches: Vec<_> = frame
                    .into_iter()
                    .flat_map(|frame| {
                        frame
                            .rig
                            .manifest
                            .iter()
                            .filter(|entry| {
                                entry.rig == rig
                                    && row["identity"].as_str()
                                        == Some(format!("{:?}", entry.identity).as_str())
                            })
                            .map(|entry| entry.instance_index)
                    })
                    .collect();
                let key = (
                    row["requested_rig"].as_u64(),
                    row["identity"].as_str().unwrap_or("").to_owned(),
                );
                let ambiguous =
                    !matches.is_empty() && (truncated || counts[&key] != 1 || matches.len() != 1);
                row["published_instance_index"] =
                    json!((!ambiguous).then(|| matches.first().copied()).flatten());
                row["publication_match_ambiguous"] = json!(ambiguous);
                row["matching_instance_indices"] = json!(matches);
            }
        }
        report["published_frame_generation"] = json!(frame.map(|frame| frame.rig.frame_generation));
        report["published_revision"] = json!(frame.map(|frame| frame.revision));
        report["geometry_revision"] = json!(frame.map(|frame| frame.rig.geometry_revision));
        report["published_instances"] = json!(frame.map(|frame| frame.rig.instances.len()));
        report["rejects"] = json!(frame.map(|frame| format!("{:?}", frame.rig.rejects)));
        self.snapshot = Some(report);
    }
}

#[cfg(test)]
#[path = "hand_inspection_tests.rs"]
mod tests;
