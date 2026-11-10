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
    /// A rule's selector evaluated once per frame for all of its assigned bones.
    selection: Option<Arc<Selection<Raster>>>,
}

/// A rule's fixed raster or shared decision tree, kept whole while assigning bones.
#[derive(Clone)]
struct Choice {
    raster: Raster,
    selection: Option<Arc<Selection<Raster>>>,
}

impl PartialEq for Choice {
    fn eq(&self, other: &Self) -> bool {
        self.raster == other.raster
            && match (&self.selection, &other.selection) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }
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
    let state = RefCell::new((&mut *table, &mut *provenance));
    let resolve_alias = |alias: &str| {
        let target = aliases.get(alias)?;
        let (table, provenance) = &mut *state.borrow_mut();
        Some(raster(Some(target), materials, table, provenance))
    };
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
        let Some(selection) = value.as_str().and_then(|text| selector.tree(text)) else {
            continue;
        };
        let choice = match selection.as_ref() {
            Selection::Empty => continue,
            Selection::Leaf(raster) => Choice {
                raster: *raster,
                selection: None,
            },
            _ => Choice {
                raster: Raster::default(),
                selection: Some(selection),
            },
        };
        rules.push((pattern.to_ascii_lowercase(), choice));
    }
    drop(state);
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
            rules.push((
                pattern.to_owned(),
                Choice {
                    raster,
                    selection: None,
                },
            ));
        }
    }
    partition(&rules, geometries, &selected, parents)
}

/// Assigns each bone to its last matching rule, preserving each selector as one decision.
fn partition(
    rules: &[(String, Choice)],
    geometries: &[EntityGeometry],
    selected: &[u32],
    parents: &mut Option<Box<[Option<usize>]>>,
) -> Result<Vec<Group>, AssetError> {
    let unmatched = Choice {
        raster: Raster::default(),
        selection: None,
    };
    let fallback = rules
        .iter()
        .rev()
        .find(|(pattern, _)| pattern == "*")
        .map_or(&unmatched, |(_, choice)| choice);
    let single = |choice: &Choice| {
        Ok(vec![Group {
            raster: choice.raster,
            hidden: Vec::new(),
            selection: choice.selection.clone(),
        }])
    };
    if rules.iter().all(|(_, choice)| choice == fallback) {
        return single(fallback);
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
        return single(fallback);
    }
    let assignments: Vec<_> = names
        .into_iter()
        .map(|name| {
            let choice = rules
                .iter()
                .rev()
                .find(|(pattern, _)| entity_render_pattern_matches(pattern, &name))
                .map_or(fallback, |(_, choice)| choice);
            (name, choice)
        })
        .collect();
    let mut order: Vec<&Choice> = Vec::new();
    for choice in rules
        .iter()
        .map(|(_, choice)| choice)
        .chain(std::iter::once(fallback))
    {
        if !order.contains(&choice) && assignments.iter().any(|(_, assigned)| *assigned == choice) {
            order.push(choice);
        }
    }
    if let [choice] = order.as_slice() {
        return single(choice);
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
    let groups: Vec<_> = order
        .into_iter()
        .map(|choice| Group {
            raster: choice.raster,
            hidden: assignments
                .iter()
                .filter(|(_, assigned)| *assigned != choice)
                .map(|(name, _)| name.as_str().into())
                .collect(),
            selection: choice.selection.clone(),
        })
        .collect();
    if groups.iter().map(|group| group.hidden.len()).sum::<usize>() > MAX_ENTITY_RENDER_VISIBILITY {
        return Err(invalid(
            "entity material bone masks exceed admitted render bounds",
        ));
    }
    Ok(groups)
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
        skipped_draws,
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
            material_selection: group.selection.clone().and_then(|selection| {
                retain_selection(
                    selection,
                    base.slot_count,
                    selections,
                    molang,
                    skipped_draws,
                )
            }),
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
        // Material ownership only hides parts; authored visibility can never be undone.
        for pattern in group.hidden {
            visibility.push(EntityRenderVisibility {
                pattern,
                condition: hidden_condition,
                hide_only: true,
            });
        }
        layer.visibility_count = (visibility.len() - layer.first_visibility as usize) as u16;
        layer.material = group.raster.material;
        layer.material_state = group.raster.state;
        layer.material_definition = group.raster.definition;
        layer.material_masks = group.raster.masks;
        layer.uv_anim = base.uv_anim.filter(|_| group.raster.uv_anim);
        layer.material_selection = group.selection.and_then(|selection| {
            retain_selection(
                selection,
                base.slot_count,
                selections,
                molang,
                skipped_draws,
            )
        });
        layers.push(layer);
    }
    Ok(())
}

/// Drops an unsupported selection without leaving partial nodes or Molang programs behind.
fn retain_selection(
    selection: Arc<Selection<Raster>>,
    textures: u16,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
    skipped_draws: &mut u32,
) -> Option<u32> {
    let start = nodes.len();
    let mark = molang.mark();
    let mut skipped = 0;
    match compile_selection(selection, textures, nodes, molang, &mut skipped) {
        Ok(root) => {
            *skipped_draws += skipped;
            Some(root)
        }
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
    textures: u16,
    nodes: &mut Vec<assets::EntityRenderMaterialSelection>,
    molang: &mut MolangCompiler,
    skipped_draws: &mut u32,
) -> Result<u32, AssetError> {
    use assets::EntityRenderMaterialSelection as Node;
    if nodes.len() >= MAX_ENTITY_RENDER_CANDIDATES {
        return Err(invalid("entity material selection count exceeds bound"));
    }
    let root = nodes.len();
    nodes.push(Node::Empty);
    let node = match selection.as_ref() {
        Selection::Empty => Node::Empty,
        Selection::Leaf(raster) if raster.required_textures > textures => {
            *skipped_draws += 1;
            Node::Empty
        }
        Selection::Leaf(raster) => Node::Raster {
            material: raster.material,
            state: raster.state,
            definition: raster.definition,
            masks: raster.masks,
            uv_anim: raster.uv_anim,
        },
        Selection::Branch(condition, yes, no) => Node::Branch {
            condition: compile_or_zero(molang, condition)?,
            yes: compile_selection(Arc::clone(yes), textures, nodes, molang, skipped_draws)?,
            no: compile_selection(Arc::clone(no), textures, nodes, molang, skipped_draws)?,
        },
        Selection::Array(index, choices) if !choices.is_empty() => Node::Array {
            index: compile_or_zero(molang, index)?,
            choices: choices
                .iter()
                .map(|choice| {
                    compile_selection(Arc::clone(choice), textures, nodes, molang, skipped_draws)
                })
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
