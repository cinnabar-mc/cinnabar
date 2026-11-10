//! Ground effects are admitted only for wearers exposed by the current actor frame.
use super::{ActorFrameInput, RemoteHat};
use bevy::prelude::Resource;
use cosmetics::Item;
use render_model::hats;

/// One admitted item bound to an actor lifetime, without account data or draw visibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroundEffectWearer {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub item: Item,
}

/// Ground items selected for visible wearers in the latest actor frame.
#[derive(Resource, Default)]
pub struct GroundEffectWearers(pub Vec<GroundEffectWearer>);

impl GroundEffectWearers {
    /// Resolves selections after body admission, skipping hidden local and culled remote wearers.
    pub(super) fn capture(
        &mut self,
        input: &ActorFrameInput,
        stream: Option<&chunk_pipeline::WorldStream>,
        batch: &crate::presentation::actors::ActorPresentationBatch,
        local_visible: bool,
    ) {
        self.0.clear();
        let Some(stream) = stream else {
            return;
        };
        let visible = |id| {
            batch.submissions.iter().any(|draw| {
                draw.input.identity.runtime_id == id
                    && draw.input.identity.layer == render_model::ACTOR_LAYER_BODY
                    && draw.route != render_model::ActorRigRoute::NoDraw
            })
        };
        let local = stream.local_player_runtime_id();
        if local_visible
            && visible(local)
            && let Some(actor) = stream.authority().actor(local)
        {
            for &item in &input.cosmetics {
                self.push(local, actor.spawn_revision, item);
            }
        }
        for &RemoteHat {
            runtime_id,
            spawn_revision,
            hat,
        } in &input.remote_hats
        {
            if runtime_id != local
                && visible(runtime_id)
                && stream
                    .authority()
                    .actor(runtime_id)
                    .is_some_and(|a| a.spawn_revision == spawn_revision && !a.is_invisible())
            {
                self.push(runtime_id, spawn_revision, hat);
            }
        }
    }

    /// Keeps each ground-only item once even if a repeated selection reaches the frame input.
    fn push(&mut self, runtime_id: u64, spawn_revision: u64, item: Item) {
        let wearer = GroundEffectWearer {
            runtime_id,
            spawn_revision,
            item,
        };
        if hats::rig(item).is_ground_effect() && !self.0.contains(&wearer) {
            self.0.push(wearer);
        }
    }
}
