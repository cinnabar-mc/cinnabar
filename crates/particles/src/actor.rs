//! Authored controller-state particle aliases, resolved against each client entity.

use std::{collections::BTreeMap, sync::Arc};

mod initialization;
pub use initialization::ActorInitialization;

use serde_json::Value;

// This legacy consumer truncation is separate from catalog admission and remains an incomplete parity limit.
const MAX_PARTICLE_OWNER_REFERENCES: usize = 512;

mod lifecycle;
pub use lifecycle::{ActorEffectKey, ActorEffectTracker};

#[derive(Clone, Debug)]
struct EffectReference {
    alias: Box<str>,
    bound: bool,
    locator: Option<Box<str>>,
    initialization: Option<Arc<ActorInitialization>>,
}

/// Particle bindings accompany the optional pack effect catalog without changing its carrier.
#[derive(Default)]
pub struct ActorEffectBindings {
    entities: BTreeMap<Box<str>, BTreeMap<Box<str>, Box<str>>>,
    controllers: BTreeMap<Box<str>, BTreeMap<Box<str>, Vec<EffectReference>>>,
    /// Authored fragments whose locator or initialization expression is not supported.
    pub unsupported: u64,
}

impl ActorEffectBindings {
    /// Replaces the winning client entity's aliases, including an explicitly empty set.
    pub fn insert_entity(&mut self, root: &Value) {
        let description = &root["minecraft:client_entity"]["description"];
        let Some(identifier) = name(&description["identifier"]) else {
            return;
        };
        if self.entities.len() >= assets::MAX_ENTITY_ASSET_SYMBOLS
            && !self.entities.contains_key(identifier)
        {
            self.unsupported += 1;
            return;
        }
        let aliases = description["particle_effects"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(alias, value)| Some((name_str(alias)?.into(), name(value)?.into())))
            .take(MAX_PARTICLE_OWNER_REFERENCES)
            .collect();
        self.entities.insert(identifier.into(), aliases);
    }

    /// Replaces each controller definition's state effects in pack priority order.
    pub fn insert_controllers(&mut self, root: &Value) {
        for (identifier, controller) in root["animation_controllers"]
            .as_object()
            .into_iter()
            .flatten()
        {
            let Some(identifier) = name_str(identifier) else {
                continue;
            };
            if self.controllers.len() >= assets::MAX_ENTITY_CONTROLLERS
                && !self.controllers.contains_key(identifier)
            {
                self.unsupported += 1;
                continue;
            }
            let mut states = BTreeMap::new();
            for (state, definition) in controller["states"]
                .as_object()
                .into_iter()
                .flatten()
                .take(assets::MAX_ENTITY_CONTROLLER_STATES)
            {
                let Some(state) = name_str(state) else {
                    continue;
                };
                let mut effects = Vec::new();
                for effect in definition["particle_effects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(MAX_PARTICLE_OWNER_REFERENCES)
                {
                    let Some(alias) = name(&effect["effect"]) else {
                        self.unsupported += 1;
                        continue;
                    };
                    let locator = match &effect["locator"] {
                        Value::Null => None,
                        Value::String(text) if text.is_empty() => None,
                        value => match name(value) {
                            Some(name) => Some(name.into()),
                            None => {
                                self.unsupported += 1;
                                continue;
                            }
                        },
                    };
                    let initialization = match &effect["pre_effect_script"] {
                        Value::Null => None,
                        Value::String(text) if text.is_empty() => None,
                        Value::String(text) => match ActorInitialization::compile(text) {
                            Some(script) => Some(Arc::new(script)),
                            None => {
                                self.unsupported += 1;
                                continue;
                            }
                        },
                        _ => {
                            self.unsupported += 1;
                            continue;
                        }
                    };
                    effects.push(EffectReference {
                        alias: alias.into(),
                        locator,
                        initialization,
                        bound: effect["bind_to_actor"].as_bool().unwrap_or(true),
                    });
                }
                states.insert(state.into(), effects);
            }
            self.controllers.insert(identifier.into(), states);
        }
    }
}

/// Reads a bounded identifier from an authored JSON string.
fn name(value: &Value) -> Option<&str> {
    name_str(value.as_str()?)
}

/// Admits a nonempty identifier within the entity symbol bound.
fn name_str(value: &str) -> Option<&str> {
    (!value.is_empty() && value.len() <= assets::MAX_ENTITY_IDENTIFIER_BYTES).then_some(value)
}

/// A resolved controller occurrence; the host supplies sampled placement and actor queries.
pub struct ActorEffect<'a> {
    pub identifier: &'a str,
    pub bound: bool,
    pub locator: Option<&'a str>,
    pub initialization: Option<&'a Arc<ActorInitialization>>,
}

impl ActorEffectBindings {
    /// Resolves typed placement and initialization references in authored occurrence order.
    pub fn effects_for<'a>(
        &'a self,
        entity: &str,
        controller: &str,
        state: &str,
    ) -> impl Iterator<Item = ActorEffect<'a>> + 'a {
        let aliases = self.entities.get(entity);
        self.controllers
            .get(controller)
            .and_then(|states| states.get(state))
            .into_iter()
            .flatten()
            .filter_map(move |effect| {
                Some(ActorEffect {
                    identifier: aliases?.get(effect.alias.as_ref())?.as_ref(),
                    bound: effect.bound,
                    locator: effect.locator.as_deref(),
                    initialization: effect.initialization.as_ref(),
                })
            })
    }
}

use render_model::ActorEmitterTransform;

/// A controller attachment belongs to this exact actor lifetime and authored locator.
#[derive(Clone, Debug)]
pub struct ActorAttachment {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub locator: Option<Box<str>>,
    pub queries: Box<[Box<str>]>,
    pub entered_tick: u64,
    /// Additional world offset for bound child emitter events.
    pub world_offset: [f32; 3],
}

#[cfg(test)]
mod coverage;

/// Separates temporary placement failure from the end of an attached actor's lifetime.
pub enum ActorAttachmentUpdate {
    /// Retains the previous placement and keeps the emitter alive.
    Pending,
    /// Ends emission because the actor is missing or replaced.
    Removed,
    /// Refreshes placement and numeric queries for the same actor lifetime.
    Placed(ActorEmitterTransform, crate::Queries),
}
