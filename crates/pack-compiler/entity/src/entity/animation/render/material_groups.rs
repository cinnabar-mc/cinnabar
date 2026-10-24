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
    hidden: Vec<Hide>,
    /// A whole-model selector evaluated once per frame in place of `raster`.
    selection: Option<Arc<Selection<Raster>>>,
}

/// A bone the group's layer hides: always, or whenever `unless` (the material-expression
/// choices that give the bone this group's material) does not hold.
struct Hide {
    pattern: Box<str>,
    unless: Option<String>,
}

/// Each material one controller rule can select, with the Molang condition selecting it;
/// a fixed material is one unconditional choice. Choices are mutually exclusive.
type Choices = Vec<(Option<String>, Raster)>;

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
                    vec![(None, raster(Some(target), materials, table, provenance))],
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
                rules.push((pattern, choices));
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
            rules.push((pattern.to_owned(), vec![(None, raster)]));
        }
    }
    partition(&rules, geometries, &selected, parents)
}

/// Partitions bones by material, the last matching rule deciding each bone. Every material a
/// rule can select gets one group; a bone whose material an expression chooses is visible in
/// each candidate group only while that group's choice holds, so the groups grow with the
/// number of choices rather than with their combinations.
fn partition(
    rules: &[(String, Choices)],
    geometries: &[EntityGeometry],
    selected: &[u32],
    parents: &mut Option<Box<[Option<usize>]>>,
) -> Result<Vec<Group>, AssetError> {
    let unmatched: Choices = vec![(None, Raster::default())];
    let fallback = rules
        .iter()
        .rev()
        .find(|(pattern, _)| pattern == "*")
        .map_or(&unmatched, |(_, choices)| choices);
    let single = |raster| {
        Ok(vec![Group {
            raster,
            hidden: Vec::new(),
            selection: None,
        }])
    };
    let first = fallback[0].1;
    if rules
        .iter()
        .flat_map(|(_, choices)| choices)
        .chain(fallback)
        .all(|(_, raster)| *raster == first)
    {
        return single(first);
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
        return single(first);
    }
    let assignments: Vec<_> = names
        .into_iter()
        .map(|name| {
            let choices = rules
                .iter()
                .rev()
                .find(|(pattern, _)| entity_render_pattern_matches(pattern, &name))
                .map_or(fallback, |(_, choices)| choices);
            (name, choices)
        })
        .collect();
    let mut order: Vec<Raster> = Vec::new();
    for (_, raster) in rules
        .iter()
        .flat_map(|(_, choices)| choices)
        .chain(fallback)
    {
        if !order.contains(raster)
            && assignments
                .iter()
                .any(|(_, choices)| choices.iter().any(|(_, assigned)| assigned == raster))
        {
            order.push(*raster);
        }
    }
    if let [raster] = order.as_slice() {
        return single(*raster);
    }
    if order.len() > MAX_ENTITY_RENDER_LAYERS {
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
    let mut hidden_total = 0usize;
    let groups: Vec<_> = order
        .into_iter()
        .map(|raster| {
            let hidden: Vec<_> = assignments
                .iter()
                .filter_map(|(name, choices)| {
                    let selecting: Vec<_> = choices
                        .iter()
                        .filter(|(_, assigned)| *assigned == raster)
                        .map(|(condition, _)| condition.as_deref())
                        .collect();
                    let unless = if selecting.is_empty() {
                        None
                    } else if selecting.len() == choices.len() || selecting.contains(&None) {
                        return None;
                    } else {
                        Some(
                            selecting
                                .iter()
                                .flatten()
                                .map(|condition| format!("({condition})"))
                                .collect::<Vec<_>>()
                                .join(" || "),
                        )
                    };
                    Some(Hide {
                        pattern: name.as_str().into(),
                        unless,
                    })
                })
                .collect();
            hidden_total = hidden_total.saturating_add(hidden.len());
            Group {
                raster,
                hidden,
                selection: None,
            }
        })
        .collect();
    if hidden_total > MAX_ENTITY_RENDER_VISIBILITY {
        return Err(invalid(
            "entity material bone masks exceed admitted render bounds",
        ));
    }
    Ok(groups)
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

/// Appends `base` once per material group, each a copy of the controller's layer drawing its
/// group's bones under the controller's activation.
pub(super) fn append(
    base: EntityRenderLayer,
    mut groups: Vec<Group>,
    records: Records<'_>,
    molang: &mut MolangCompiler,
) -> Result<(), AssetError> {
    // A material whose controller supplies too few textures is not drawn, as on Bedrock; its
    // bones stay hidden in the other groups.
    let before = groups.len();
    groups.retain(|group| group.raster.required_textures <= base.slot_count);
    *records.skipped_draws += (before - groups.len()) as u32;
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
    let added_visibility = groups.iter().map(|group| group.hidden.len()).sum::<usize>()
        + extra * usize::from(base.visibility_count);
    if layers.len() + groups.len() > MAX_ENTITY_RENDER_LAYERS
        || slots.len() + extra * usize::from(base.slot_count) > MAX_ENTITY_RENDER_SLOTS
        || candidates.len() + extra * candidate_count > MAX_ENTITY_RENDER_CANDIDATES
        || geometries.len() + extra * usize::from(base.geometry_count)
            > MAX_ENTITY_RENDER_CANDIDATES
        || visibility.len() + added_visibility > MAX_ENTITY_RENDER_VISIBILITY
        || groups.iter().any(|group| {
            usize::from(base.visibility_count) + group.hidden.len() > usize::from(u16::MAX)
        })
    {
        return Err(invalid(
            "entity material groups exceed admitted render bounds",
        ));
    }
    if let [group] = groups.as_slice()
        && group.hidden.is_empty()
    {
        layers.push(EntityRenderLayer {
            material: group.raster.material,
            material_state: group.raster.state,
            material_definition: group.raster.definition,
            material_masks: group.raster.masks,
            uv_anim: base.uv_anim.filter(|_| group.raster.uv_anim),
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
    for (index, group) in groups.into_iter().enumerate() {
        let mut layer = base;
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
        // Material ownership only hides parts; authored visibility can never be undone. A
        // chosen bone hides while its choice does not hold; one whose choice does not compile
        // stays hidden, as vanilla evaluates it as 0.0.
        for hide in group.hidden {
            let choice = hide.unless.and_then(|text| molang.compile(&text).ok());
            visibility.push(EntityRenderVisibility {
                pattern: hide.pattern,
                condition: choice.unwrap_or(hidden_condition),
                hide_only: choice.is_some(),
            });
        }
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
            condition: compile_or_zero(molang, condition)?,
            yes: compile_selection(Arc::clone(yes), nodes, molang)?,
            no: compile_selection(Arc::clone(no), nodes, molang)?,
        },
        Selection::Array(index, choices) if !choices.is_empty() => Node::Array {
            index: compile_or_zero(molang, index)?,
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

/// Compiles a selector step; one that does not compile reads 0.0, as vanilla evaluates it, so a
/// ternary takes its else branch and an array index selects the first element.
fn compile_or_zero(molang: &mut MolangCompiler, text: &str) -> Result<u32, AssetError> {
    molang.compile(text).or_else(|_| molang.compile("0.0"))
}

#[cfg(test)]
#[path = "material_groups_tests.rs"]
mod tests;
