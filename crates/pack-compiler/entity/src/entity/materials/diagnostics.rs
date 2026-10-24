//! Counted entity material behaviour the client skips, ignores or draws approximately.
//!
//! Counts are gathered once per compilation from the admitted declarations and resolved
//! materials, so a pack reports them once per generation and never per draw.

use std::collections::BTreeSet;

use assets::entity::material::{EntityMaterialState, actor_define_bit};
use serde_json::{Map, Value};

use super::{
    MaterialStates, Resolved, actor_type, actor_type::ActorMaterialType, list::MaterialListReport,
};

/// One compiled pack's entity material counts.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EntityMaterialDiagnostics {
    /// Distinct state names no Bedrock state matches. Bedrock asserts on them; the client
    /// ignores them.
    pub unknown_states: u32,
    /// Distinct define names Bedrock gives no actor flag. It ignores them without a log, and so
    /// does the client.
    pub unknown_defines: u32,
    /// `samplerStates` entries whose `samplerIndex` is 8 or more, skipped as Bedrock skips them.
    pub skipped_samplers: u32,
    /// Declared materials that do not resolve: a parent missing from their file, a parent
    /// cycle, or a malformed state or define list.
    pub failed_materials: u32,
    /// Resolved materials whose program has no actor material type; they draw as plain `Actor`.
    pub unassignable_programs: u32,
    /// Resolved materials with stencil, polygon-offset or depth-bias state the client carries
    /// without rendering.
    pub unsupported_materials: u32,
    /// Resolved materials drawn approximately: actor defines the client carries without
    /// drawing, or the `pattern` type drawn as `entity`.
    pub approximated_materials: u32,
    /// Material draws dropped because their render controller supplies too few textures.
    pub skipped_draws: u32,
    /// `materials/*.material` files no list entry named, loaded in place of the vanilla lists.
    pub unlisted_files: u32,
    /// List entries the client read in part or loaded nothing from.
    pub list: MaterialListReport,
}

impl EntityMaterialDiagnostics {
    /// Whether any count is non-zero.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Names and entries noticed while admitting declarations, before resolution.
#[derive(Default)]
pub(super) struct DeclarationNotes {
    unknown_states: BTreeSet<Box<str>>,
    unknown_defines: BTreeSet<Box<str>>,
    skipped_samplers: u32,
    pub(super) unlisted_files: u32,
    pub(super) list: MaterialListReport,
}

impl DeclarationNotes {
    /// Notes the unknown names and skipped sampler entries of one declaration, its variants
    /// included.
    pub(super) fn declaration(&mut self, fields: &Map<String, Value>) {
        for key in ["states", "+states", "-states"] {
            for name in strings(fields, key) {
                if EntityMaterialState::from_name(name).is_none() {
                    self.unknown_states.insert(name.into());
                }
            }
        }
        for key in ["defines", "+defines", "-defines"] {
            self.defines(strings(fields, key));
        }
        for key in ["samplerStates", "+samplerStates"] {
            self.skipped_samplers += fields
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|entry| {
                    entry
                        .get("samplerIndex")
                        .and_then(Value::as_u64)
                        .is_some_and(|slot| slot >= 8)
                })
                .count() as u32;
        }
        for key in ["variants", "+variants"] {
            for variant in fields
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .flat_map(Map::values)
                .filter_map(Value::as_object)
            {
                self.declaration(variant);
            }
        }
    }

    /// Notes define names, such as a list entry's, that set no actor flag.
    pub(super) fn defines<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        for name in names {
            if actor_define_bit(name).is_none() {
                self.unknown_defines.insert(name.into());
            }
        }
    }

    /// Adds `other`'s notes to these.
    pub(super) fn extend(&mut self, other: Self) {
        self.unknown_states.extend(other.unknown_states);
        self.unknown_defines.extend(other.unknown_defines);
        self.skipped_samplers += other.skipped_samplers;
        self.unlisted_files += other.unlisted_files;
        self.list.add(other.list);
    }
}

fn strings<'a>(fields: &'a Map<String, Value>, key: &str) -> impl Iterator<Item = &'a str> {
    fields
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

impl MaterialStates {
    /// Counts this compilation's material behaviour, resolving every declared material once
    /// as an actor draws it.
    pub(crate) fn diagnostics(&self) -> EntityMaterialDiagnostics {
        let notes = &self.notes;
        let mut diagnostics = EntityMaterialDiagnostics {
            unknown_states: notes.unknown_states.len() as u32,
            unknown_defines: notes.unknown_defines.len() as u32,
            skipped_samplers: notes.skipped_samplers,
            unlisted_files: notes.unlisted_files,
            list: notes.list,
            ..EntityMaterialDiagnostics::default()
        };
        let mut memo = super::ChainMemo::default();
        for name in self.definitions.keys() {
            let Some(resolved) = self.resolve_with(name, &super::builtin, &mut memo) else {
                diagnostics.failed_materials += 1;
                continue;
            };
            let drawn = resolved.actor_variant();
            let kind = actor_type::of(drawn);
            diagnostics.unassignable_programs += u32::from(kind == ActorMaterialType::Other);
            diagnostics.unsupported_materials += u32::from(unsupported(drawn));
            diagnostics.approximated_materials += u32::from(
                kind == ActorMaterialType::Pattern
                    || drawn.defines.iter().any(|define| {
                        actor_define_bit(define).is_some()
                            && !super::APPLIED_DEFINES.contains(&define.as_ref())
                    }),
            );
        }
        diagnostics
    }
}

/// Whether `resolved` carries state the client does not render for actors.
fn unsupported(resolved: &Resolved) -> bool {
    use EntityMaterialState as State;
    [
        State::StencilWrite,
        State::EnableStencilTest,
        State::PolygonOffset,
    ]
    .iter()
    .any(|state| resolved.states.contains(state))
        || resolved.definition.depth_bias != 0.0
        || resolved.definition.slope_scaled_depth_bias != 0.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::list::ENTITY_MATERIAL_LIST_PATH;
    use super::*;

    fn modern(materials: Value) -> Value {
        let mut materials = materials;
        materials["version"] = json!("1.0.0");
        json!({ "materials": materials })
    }

    #[test]
    fn counts_are_aggregated_once_per_compilation() {
        let listed = modern(json!({
            "plain": {"fragmentShader": "shaders/entity.fragment", "+states": ["Blending", "NotAState"], "+defines": ["NOT_A_FLAG", "USE_OVERLAY"]},
            "repeat": {"fragmentShader": "shaders/entity.fragment", "+states": ["NotAState"], "+defines": ["NOT_A_FLAG"]},
            "sampled": {"fragmentShader": "shaders/entity.fragment", "samplerStates": [{"samplerIndex": 7}, {"samplerIndex": 8}, {"samplerIndex": 12}]},
            "glint": {"fragmentShader": "shaders/entity.fragment", "+defines": ["GLINT"]},
            "pattern": {"fragmentShader": "shaders/pattern.fragment"},
            "custom": {"fragmentShader": "shaders/custom.fragment"},
            "stencil:entity": {"+states": ["EnableStencilTest"]},
            "biased:entity": {"depthBias": 2.0},
            "orphan:missing": {},
            "variant:entity": {"variants": [{"skinning": {"+defines": ["SKIN_ONLY"]}}]}
        }));
        let documents = [
            (
                ENTITY_MATERIAL_LIST_PATH,
                json!([{"path": "materials/listed.material", "tag": "old", "+defines": ["LIST_ONLY"]}, {"path": ""}]),
            ),
            ("materials/listed.material", listed),
            ("materials/unlisted.material", modern(json!({}))),
        ];
        let states =
            MaterialStates::from_documents(documents.iter().map(|(path, value)| (*path, value)));
        let diagnostics = states.diagnostics();
        assert_eq!(
            diagnostics,
            EntityMaterialDiagnostics {
                unknown_states: 1,
                unknown_defines: 3,
                skipped_samplers: 2,
                failed_materials: 1,
                unassignable_programs: 1,
                unsupported_materials: 2,
                approximated_materials: 2,
                skipped_draws: 0,
                unlisted_files: 1,
                list: MaterialListReport {
                    ignored_tags: 1,
                    empty_paths: 1,
                    ..MaterialListReport::default()
                },
            }
        );
        assert_eq!(states.diagnostics(), diagnostics, "resolution adds nothing");
    }
}
