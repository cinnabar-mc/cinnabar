use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{
    AssetError, EntityGeometry, EntityRenderCandidate, EntityRenderGeometry, EntityRenderLayer,
    EntityRenderMaterial, EntityRenderMaterialState, EntityRenderSlot, EntityRenderVisibility,
    MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS, MAX_ENTITY_RENDER_PATTERN_BYTES,
    MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY, entity_render_pattern_matches,
    validate_entity_geometry_inheritance,
};
use serde_json::{Map, Value};

use super::{
    super::selection::Selection,
    Selector,
    materials::{MaterialStates, MaterialTable, provenance::MaterialProvenance},
    selection_condition,
};
use crate::entity::{invalid, molang::MolangCompiler};

#[derive(Clone, Copy, PartialEq)]
struct Raster {
    material: EntityRenderMaterial,
    state: Option<EntityRenderMaterialState>,
    /// Index of the complete definition in the carrier's material table.
    definition: Option<u32>,
    /// Index of a Cinnabar material's masks in the carrier's mask table.
    masks: Option<u32>,
    /// Textures the controller must supply for this material to draw.
    required_textures: u16,
    /// Whether the material applies the controller's `uv_anim`.
    uv_anim: bool,
}

impl Default for Raster {
    /// A layer naming no material, which applies its controller's `uv_anim`.
    fn default() -> Self {
        Self {
            material: EntityRenderMaterial::default(),
            state: None,
            definition: None,
            masks: None,
            required_textures: 0,
            uv_anim: true,
        }
    }
}

pub(super) struct Group {
    raster: Raster,
    hidden: Vec<Box<str>>,
    selection: Option<Arc<Selection<Raster>>>,
    /// Molang that must hold for the group to draw: the material choices of the controller's
    /// expression rules that produced it. `None` always draws.
    condition: Option<String>,
}

/// Most combinations of expression-rule choices one controller expands into; beyond it each
/// expression keeps the material a default actor selects.
const MAX_MATERIAL_VARIANTS: usize = 16;

/// One controller material rule: a fixed material, or each material an expression can select
/// with the Molang condition selecting it.
enum Rule {
    Fixed(Raster),
    Choice(Vec<(Option<String>, Raster)>),
}

/// The shader kind, shading features and interned definition one controller material selects.
fn raster(
    target: Option<&str>,
    materials: &MaterialStates,
    table: &mut MaterialTable,
    provenance: &mut MaterialProvenance,
) -> Raster {
    let resolved = target.and_then(|target| materials.resolve(target, table));
    if let (Some(target), Some((_, definition))) = (target, resolved) {
        materials.record_provenance(target, definition, table, provenance);
    }
    Raster {
        material: target.map_or(EntityRenderMaterial::Default, |name| {
            assets::facts::pack::rules().raster(name)
        }),
        state: resolved.map(|(state, _)| state),
        definition: resolved.map(|(_, definition)| definition),
        masks: None,
        required_textures: resolved
            .map_or(0, |(_, definition)| table.required_textures(definition)),
        uv_anim: target.is_none_or(|target| materials.applies_uv_anim(target, table)),
    }
}

/// Partitions a controller's bones by material: its authored rules first, then Cinnabar
/// extension bindings for (`entity`, `controller_name`), the last match winning per bone.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve(
    (entity, controller_name): (&str, &str),
    description: &Map<String, Value>,
    controller: &Map<String, Value>,
    materials: &MaterialStates,
    (table, provenance): (&mut MaterialTable, &mut MaterialProvenance),
    geometries: &[EntityGeometry],
    selected: impl IntoIterator<Item = u32>,
    parents: &mut Option<Box<[Option<usize>]>>,
) -> Result<Vec<Group>, AssetError> {
    // A whole-model selector draws one layer; extension bindings partition bones instead.
    let extended = materials
        .extension_rules(entity, controller_name)
        .next()
        .is_some();
    if !extended
        && let Some(selection) = uniform_selection(
            description,
            controller,
            materials,
            (&mut *table, &mut *provenance),
        )?
    {
        return Ok(vec![Group {
            raster: Raster::default(),
            hidden: Vec::new(),
            selection: Some(selection),
            condition: None,
        }]);
    }
    let selected = selected.into_iter().collect::<Vec<_>>();
    let aliases = super::lowercase_map(description.get("materials"));
    let arrays = controller
        .get("arrays")
        .and_then(|arrays| arrays.get("materials"))
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, members)| {
            (
                name.to_ascii_lowercase(),
                members
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|member| member.as_str().map(str::to_owned))
                    .collect(),
            )
        })
        .collect();
    let resolve_alias = |alias: &str| aliases.get(alias).cloned();
    let selector = Selector {
        prefix: "material.",
        arrays,
        resolve: &resolve_alias,
    };
    let mut rules = Vec::new();
    for (pattern, value) in controller
        .get("materials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flatten()
    {
        let Some(leaves) = value.as_str().and_then(|text| selector.leaves(text)) else {
            continue;
        };
        let pattern = pattern.to_ascii_lowercase();
        match leaves.as_slice() {
            [] => {}
            [(path, target)] if path.is_empty() => {
                rules.push((
                    pattern,
                    Rule::Fixed(raster(Some(target), materials, table, provenance)),
                ));
            }
            _ => {
                let mut choices = Vec::with_capacity(leaves.len());
                for (path, target) in &leaves {
                    choices.push((
                        selection_condition(path),
                        raster(Some(target), materials, table, provenance),
                    ));
                }
                rules.push((pattern, Rule::Choice(choices)));
            }
        }
    }
    for (pattern, material) in materials.extension_rules(entity, controller_name) {
        if let Some((state, definition, masks)) = materials.resolve_extension(material, true, table)
        {
            materials.record_extension_provenance(material, definition, table, provenance);
            let raster = Raster {
                material: EntityRenderMaterial::Default,
                state: Some(state),
                definition: Some(definition),
                masks,
                required_textures: table.required_textures(definition),
                uv_anim: materials.extension_applies_uv_anim(material, table),
            };
            rules.push((pattern.to_owned(), Rule::Fixed(raster)));
        }
    }
    let mut groups = Vec::new();
    for (condition, rules) in variants(&rules) {
        for mut group in partition(&rules, geometries, &selected, parents)? {
            group.condition = condition.clone();
            groups.push(group);
        }
    }
    Ok(groups)
}

/// One combination of expression-rule choices: its selecting condition and fixed rules.
type Variant = (Option<String>, Vec<(String, Raster)>);

/// Every combination of the expression rules' choices, as fixed rules with the condition that
/// selects the combination. Choices are mutually exclusive, so exactly one combination draws.
fn variants(rules: &[(String, Rule)]) -> Vec<Variant> {
    let count = rules
        .iter()
        .try_fold(1usize, |count, (_, rule)| match rule {
            Rule::Fixed(_) => Some(count),
            Rule::Choice(choices) => count.checked_mul(choices.len()),
        });
    let Some(count) = count.filter(|count| *count <= MAX_MATERIAL_VARIANTS) else {
        // Too many combinations: each expression keeps what a default actor selects.
        let fixed = rules
            .iter()
            .filter_map(|(pattern, rule)| {
                let raster = match rule {
                    Rule::Fixed(raster) => *raster,
                    Rule::Choice(choices) => {
                        choices
                            .iter()
                            .find(|(condition, _)| {
                                condition.as_deref().is_none_or(|text| {
                                    MolangCompiler::evaluate_default(text)
                                        .is_some_and(|value| value != 0.0)
                                })
                            })?
                            .1
                    }
                };
                Some((pattern.clone(), raster))
            })
            .collect();
        return vec![(None, fixed)];
    };
    (0..count)
        .map(|mut index| {
            let mut conditions = Vec::new();
            let fixed = rules
                .iter()
                .map(|(pattern, rule)| {
                    let raster = match rule {
                        Rule::Fixed(raster) => *raster,
                        Rule::Choice(choices) => {
                            let (condition, raster) = &choices[index % choices.len()];
                            index /= choices.len();
                            conditions.extend(condition.clone());
                            *raster
                        }
                    };
                    (pattern.clone(), raster)
                })
                .collect();
            let condition = (!conditions.is_empty()).then(|| {
                conditions
                    .iter()
                    .map(|condition| format!("({condition})"))
                    .collect::<Vec<_>>()
                    .join(" && ")
            });
            (condition, fixed)
        })
        .collect()
}

/// Partitions bones by fixed material rules, the last match winning per bone.
fn partition(
    rules: &[(String, Raster)],
    geometries: &[EntityGeometry],
    selected: &[u32],
    parents: &mut Option<Box<[Option<usize>]>>,
) -> Result<Vec<Group>, AssetError> {
    let fallback = rules
        .iter()
        .rev()
        .find(|(pattern, _)| pattern == "*")
        .map_or_else(Raster::default, |(_, raster)| *raster);
    if rules.iter().all(|(_, raster)| *raster == fallback) {
        return Ok(vec![Group {
            raster: fallback,
            hidden: Vec::new(),
            selection: None,
            condition: None,
        }]);
    }
    if parents.is_none() {
        *parents = Some(validate_entity_geometry_inheritance(geometries)?);
    }
    let parents = parents.as_deref().expect("resolved geometry parents");
    let mut names = BTreeSet::new();
    for &selected in selected {
        let mut current = Some(selected as usize);
        while let Some(index) = current {
            let geometry = &geometries[index];
            names.extend(
                geometry
                    .bones
                    .iter()
                    .map(|bone| bone.name.to_ascii_lowercase()),
            );
            current = parents[index];
        }
    }
    if names.is_empty() {
        return Ok(vec![Group {
            raster: fallback,
            hidden: Vec::new(),
            selection: None,
            condition: None,
        }]);
    }
    let assignments: Vec<_> = names
        .into_iter()
        .map(|name| {
            let raster = rules
                .iter()
                .rev()
                .find(|(pattern, _)| entity_render_pattern_matches(pattern, &name))
                .map_or(fallback, |(_, raster)| *raster);
            (name, raster)
        })
        .collect();
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for raster in rules.iter().map(|(_, raster)| *raster).chain([fallback]) {
        if assignments.iter().any(|(_, assigned)| *assigned == raster)
            && seen.insert((
                raster.material.word(raster.state),
                raster.definition,
                raster.masks,
            ))
        {
            order.push(raster);
        }
    }
    if order.len() == 1 {
        return Ok(vec![Group {
            raster: order[0],
            hidden: Vec::new(),
            selection: None,
            condition: None,
        }]);
    }
    if order.len() > MAX_ENTITY_RENDER_LAYERS
        || assignments.len().saturating_mul(order.len() - 1) > MAX_ENTITY_RENDER_VISIBILITY
    {
        return Err(invalid(
            "entity material bone masks exceed admitted render bounds",
        ));
    }
    if assignments
        .iter()
        .any(|(name, _)| name.len() > MAX_ENTITY_RENDER_PATTERN_BYTES || name.contains('*'))
    {
        return Err(invalid(
            "entity material bone mask exceeds admitted pattern bounds",
        ));
    }
    Ok(order
        .into_iter()
        .map(|raster| Group {
            raster,
            selection: None,
            hidden: assignments
                .iter()
                .filter(|(_, assigned)| *assigned != raster)
                .map(|(name, _)| name.as_str().into())
                .collect(),
            condition: None,
        })
        .collect())
}

/// Expands a whole-model material selector without rewriting authored part visibility.
/// Choices resolve and intern their complete definitions as the selector reaches them.
fn uniform_selection(
    description: &Map<String, Value>,
    controller: &Map<String, Value>,
    materials: &MaterialStates,
    (table, provenance): (&mut MaterialTable, &mut MaterialProvenance),
) -> Result<Option<Arc<Selection<Raster>>>, AssetError> {
    use super::super::selection::Selector;
    let Some(expression) = controller
        .get("materials")
        .and_then(Value::as_array)
        .and_then(|rules| match rules.as_slice() {
            [rule] => rule.as_object().filter(|rule| rule.len() == 1),
            _ => None,
        })
        .and_then(|rule| rule.get("*")?.as_str())
    else {
        return Ok(None);
    };
    let aliases = super::lowercase_map(description.get("materials"));
    let state = RefCell::new((table, provenance));
    let resolve_alias = |alias: &str| {
        let target = aliases.get(alias)?;
        let (table, provenance) = &mut *state.borrow_mut();
        Some(raster(Some(target), materials, table, provenance))
    };
    let selector = Selector {
        prefix: "material.",
        arrays: controller
            .get("arrays")
            .and_then(|arrays| arrays.get("materials"))
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, values)| {
                (
                    name.to_ascii_lowercase(),
                    values
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|value| value.as_str().map(str::to_owned))
                        .collect(),
                )
            })
            .collect(),
        resolve: &resolve_alias,
    };
    let selection = selector.tree(expression);
    Ok(selection.filter(|selection| {
        matches!(
            selection.as_ref(),
            Selection::Branch(..) | Selection::Array(..)
        )
    }))
}

pub(super) struct Records<'a> {
    pub selections: &'a mut Vec<assets::EntityRenderMaterialSelection>,
    pub layers: &'a mut Vec<EntityRenderLayer>,
    pub slots: &'a mut Vec<EntityRenderSlot>,
    pub candidates: &'a mut Vec<EntityRenderCandidate>,
    pub visibility: &'a mut Vec<EntityRenderVisibility>,
    pub geometries: &'a mut Vec<EntityRenderGeometry>,
    /// Material draws dropped because the controller supplies too few textures.
    pub skipped_draws: &'a mut u32,
}

pub(super) fn append(
    base: EntityRenderLayer,
    activation: Option<&str>,
    mut groups: Vec<Group>,
    records: Records<'_>,
    molang: &mut MolangCompiler,
) -> Result<(), AssetError> {
    // A material whose controller supplies too few textures is not drawn, as on Bedrock; its
    // bones stay hidden in the other groups.
    let before = groups.len();
    groups.retain(|group| group.raster.required_textures <= base.slot_count);
    *records.skipped_draws += (before - groups.len()) as u32;
    // A group selected by a material expression draws only while its choice and the
    // controller's activation both hold; one whose condition does not compile is not drawn.
    let groups = groups
        .into_iter()
        .filter_map(|group| {
            let condition = match &group.condition {
                None => base.condition,
                Some(choice) => {
                    let text = activation.map_or_else(
                        || choice.clone(),
                        |activation| format!("({activation}) && ({choice})"),
                    );
                    Some(molang.compile(&text).ok()?)
                }
            };
            Some((group, condition))
        })
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return Ok(());
    }
    let Records {
        selections,
        layers,
        slots,
        candidates,
        visibility,
        geometries,
        skipped_draws: _,
    } = records;
    let extra = groups.len().saturating_sub(1);
    let candidate_count: usize = slots[base.first_slot as usize..]
        .iter()
        .map(|slot| usize::from(slot.candidate_count))
        .sum();
    let added_visibility = groups
        .iter()
        .map(|(group, _)| group.hidden.len())
        .sum::<usize>()
        + extra * usize::from(base.visibility_count);
    if layers.len() + groups.len() > MAX_ENTITY_RENDER_LAYERS
        || slots.len() + extra * usize::from(base.slot_count) > MAX_ENTITY_RENDER_SLOTS
        || candidates.len() + extra * candidate_count > MAX_ENTITY_RENDER_CANDIDATES
        || geometries.len() + extra * usize::from(base.geometry_count)
            > MAX_ENTITY_RENDER_CANDIDATES
        || visibility.len() + added_visibility > MAX_ENTITY_RENDER_VISIBILITY
        || groups.iter().any(|(group, _)| {
            usize::from(base.visibility_count) + group.hidden.len() > usize::from(u16::MAX)
        })
    {
        return Err(invalid(
            "entity material groups exceed admitted render bounds",
        ));
    }
    if let [(group, condition)] = groups.as_slice()
        && group.hidden.is_empty()
    {
        layers.push(EntityRenderLayer {
            material: group.raster.material,
            material_state: group.raster.state,
            material_definition: group.raster.definition,
            material_masks: group.raster.masks,
            uv_anim: base.uv_anim.filter(|_| group.raster.uv_anim),
            condition: *condition,
            material_selection: group
                .selection
                .clone()
                .and_then(|selection| retain_selection(selection, selections, molang)),
            ..base
        });
        return Ok(());
    }
    let hidden_condition = molang.compile("0.0")?;
    let authored_visibility = visibility[base.first_visibility as usize..].to_vec();
    let authored_slots = slots[base.first_slot as usize..].to_vec();
    let authored_candidates: BTreeMap<_, _> = authored_slots
        .iter()
        .map(|slot| {
            let first = slot.first_candidate as usize;
            (
                slot.first_candidate,
                candidates[first..first + usize::from(slot.candidate_count)].to_vec(),
            )
        })
        .collect();
    let authored_geometries = geometries[base.first_geometry as usize..].to_vec();
    for (index, (group, condition)) in groups.into_iter().enumerate() {
        let mut layer = EntityRenderLayer { condition, ..base };
        if index != 0 {
            layer.first_slot = slots.len() as u32;
            for slot in &authored_slots {
                let first_candidate = candidates.len() as u32;
                candidates.extend_from_slice(&authored_candidates[&slot.first_candidate]);
                slots.push(EntityRenderSlot {
                    first_candidate,
                    ..*slot
                });
            }
            layer.first_geometry = geometries.len() as u32;
            geometries.extend_from_slice(&authored_geometries);
            layer.first_visibility = visibility.len() as u32;
            visibility.extend_from_slice(&authored_visibility);
        }
        // Material ownership only hides parts; authored visibility can never be undone.
        visibility.extend(
            group
                .hidden
                .into_iter()
                .map(|pattern| EntityRenderVisibility {
                    pattern,
                    condition: hidden_condition,
                }),
        );
        layer.visibility_count = (visibility.len() - layer.first_visibility as usize) as u16;
        layer.material = group.raster.material;
        layer.material_state = group.raster.state;
        layer.material_definition = group.raster.definition;
        layer.material_masks = group.raster.masks;
        layer.uv_anim = base.uv_anim.filter(|_| group.raster.uv_anim);
        layer.material_selection = group
            .selection
            .and_then(|selection| retain_selection(selection, selections, molang));
        layers.push(layer);
    }
    Ok(())
}

/// Drops an unsupported selection without leaving partial nodes or Molang programs behind.
fn retain_selection(
    selection: Arc<Selection<Raster>>,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
) -> Option<u32> {
    let start = nodes.len();
    let mark = molang.mark();
    match compile_selection(selection, nodes, molang) {
        Ok(root) => Some(root),
        Err(_) => {
            nodes.truncate(start);
            molang.rollback(mark);
            None
        }
    }
}

/// Lowers one decision tree without cloning the authored controller or repeating its scripts.
fn compile_selection(
    selection: Arc<Selection<Raster>>,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
) -> Result<u32, AssetError> {
    use assets::EntityRenderMaterialSelection as Node;
    if nodes.len() >= MAX_ENTITY_RENDER_CANDIDATES {
        return Err(invalid("entity material selection count exceeds bound"));
    }
    let root = nodes.len();
    nodes.push(Node::Raster {
        material: EntityRenderMaterial::Default,
        state: None,
        definition: None,
        masks: None,
    });
    let node = match selection.as_ref() {
        Selection::Empty => Node::Raster {
            material: EntityRenderMaterial::Default,
            state: None,
            definition: None,
            masks: None,
        },
        Selection::Leaf(raster) => Node::Raster {
            material: raster.material,
            state: raster.state,
            definition: raster.definition,
            masks: raster.masks,
        },
        Selection::Branch(condition, yes, no) => Node::Branch {
            condition: molang.compile(condition)?,
            yes: compile_selection(Arc::clone(yes), nodes, molang)?,
            no: compile_selection(Arc::clone(no), nodes, molang)?,
        },
        Selection::Array(index, choices) if !choices.is_empty() => Node::Array {
            index: molang.compile(index)?,
            choices: choices
                .iter()
                .map(|choice| compile_selection(Arc::clone(choice), nodes, molang))
                .collect::<Result<_, _>>()?,
        },
        Selection::Array(..) => return Err(invalid("empty material selector array")),
    };
    nodes[root] = node;
    Ok(root as u32)
}

#[cfg(test)]
#[path = "material_groups_tests.rs"]
mod tests;
