//! Where each material an actor draws came from and how it draws, for reports and inspectors.

use std::collections::BTreeMap;

use assets::{
    entity::material::actor_define_bit,
    entity::material_report::{EntityMaterialChain, EntityMaterialProvenance},
};

use super::{
    APPLIED_DEFINES, MaterialStates, MaterialTable, Resolved, actor_type,
    actor_type::ActorMaterialType, builtin, diagnostics::unsupported,
    extension::MATERIAL_BINDINGS_PATH, material_name,
};

/// The source a client-declared base reports.
const CLIENT_BASE: &str = "client base";

/// The materials a compilation's actors draw, by name.
#[derive(Default)]
pub(crate) struct MaterialProvenance {
    materials: BTreeMap<Box<str>, EntityMaterialProvenance>,
    chains: BTreeMap<super::ChainLink, EntityMaterialChain>,
}

impl MaterialProvenance {
    /// The recorded materials, sorted by name.
    pub(crate) fn finish(self) -> Box<[EntityMaterialProvenance]> {
        self.materials.into_values().collect()
    }
}

impl MaterialStates {
    /// Records how client-entity material `target` draws with table index `definition`,
    /// resolving through `table`'s remembered parent chains.
    pub(crate) fn record_provenance(
        &self,
        target: &str,
        definition: u32,
        table: &mut MaterialTable,
        provenance: &mut MaterialProvenance,
    ) {
        if provenance.materials.contains_key(target) {
            return;
        }
        let name = material_name(target);
        let Some(resolved) = self.resolve_with(name, &builtin, &mut table.chains) else {
            return;
        };
        let (source, chain) = self.chain(name, &mut provenance.chains);
        provenance.materials.insert(
            target.into(),
            describe(target, definition, source, chain, &resolved),
        );
    }

    /// Records how extension material `name` draws with table index `definition`, resolving
    /// through `table`'s remembered parent chains.
    pub(crate) fn record_extension_provenance(
        &self,
        name: &str,
        definition: u32,
        table: &mut MaterialTable,
        provenance: &mut MaterialProvenance,
    ) {
        if provenance.materials.contains_key(name) {
            return;
        }
        let Some((material, resolved)) = self.extension_material(name, &mut table.chains) else {
            return;
        };
        let parent = material
            .base
            .as_deref()
            .map_or_else(EntityMaterialChain::default, |base| {
                self.chain(material_name(base), &mut provenance.chains).1
            });
        let chain = EntityMaterialChain::prepend(name.into(), parent);
        provenance.materials.insert(
            name.into(),
            describe(
                name,
                definition,
                MATERIAL_BINDINGS_PATH.into(),
                chain,
                &resolved,
            ),
        );
    }

    /// Returns the declaring file and shared ancestry, following the resolver's file retries.
    fn chain(
        &self,
        name: &str,
        memo: &mut BTreeMap<super::ChainLink, EntityMaterialChain>,
    ) -> (Box<str>, EntityMaterialChain) {
        let source = self
            .definitions
            .get(name)
            .and_then(|declaration| self.file_paths.get(declaration.file))
            .cloned()
            .unwrap_or_else(|| CLIENT_BASE.into());
        let mut pending = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let (mut current, mut file) = (name, None);
        let mut chain = loop {
            let key = (Box::<str>::from(current), file);
            if let Some(known) = memo.get(&key) {
                break known.clone();
            }
            if !visited.insert((current, file)) {
                break EntityMaterialChain::default();
            }
            let declaration = self
                .definitions
                .get(current)
                .filter(|declaration| file.is_none_or(|file| declaration.file == file));
            if declaration.is_none()
                && file.is_some()
                && self.definitions.contains_key(current)
                && builtin(current).is_some()
            {
                pending.push((key, None));
                file = None;
                continue;
            }
            let label = if declaration.is_none() && builtin(current).is_some() {
                format!("{current} ({CLIENT_BASE})").into_boxed_str()
            } else {
                current.into()
            };
            pending.push((key, Some(label)));
            let Some(declaration) = declaration else {
                break EntityMaterialChain::default();
            };
            let Some(parent) = declaration.parent.as_deref() else {
                break EntityMaterialChain::default();
            };
            file = Some(declaration.file);
            current = parent;
        };
        for (key, label) in pending.into_iter().rev() {
            if let Some(label) = label {
                chain = EntityMaterialChain::prepend(label, chain);
            }
            memo.insert(key, chain.clone());
        }
        (source, chain)
    }
}

/// The provenance of a resolved material as an actor draws it.
fn describe(
    name: &str,
    definition: u32,
    source: Box<str>,
    chain: EntityMaterialChain,
    resolved: &Resolved,
) -> EntityMaterialProvenance {
    let drawn = resolved.actor_variant();
    let variant = if std::ptr::eq(drawn, resolved) {
        "base"
    } else {
        super::SKINNING_VARIANT
    };
    let (applied, carried): (Vec<_>, Vec<_>) = drawn
        .defines
        .iter()
        .cloned()
        .partition(|define| APPLIED_DEFINES.contains(&define.as_ref()));
    let undrawn = carried
        .iter()
        .filter(|define| actor_define_bit(define).is_some())
        .map(AsRef::as_ref)
        .collect::<Vec<&str>>();
    let mut reasons = Vec::new();
    match actor_type::of(drawn) {
        ActorMaterialType::Other => {
            reasons.push("program has no actor material type; drawn as plain Actor".to_owned());
        }
        ActorMaterialType::Pattern => reasons.push("pattern program drawn as entity".to_owned()),
        ActorMaterialType::Entity => {}
    }
    if unsupported(drawn) {
        reasons.push("stencil, polygon offset or depth bias is not rendered".to_owned());
    }
    if !undrawn.is_empty() {
        reasons.push(format!("defines not drawn: {}", undrawn.join(", ")));
    }
    EntityMaterialProvenance {
        name: name.into(),
        definition,
        source,
        chain,
        variant: variant.into(),
        applied_defines: applied.into(),
        carried_defines: carried.into(),
        reason: (!reasons.is_empty()).then(|| reasons.join("; ").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[global_allocator]
    static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

    #[test]
    fn reporting_every_link_of_a_long_chain_allocates_linear_space() {
        const COUNT: usize = 1024;
        let mut declarations = serde_json::Map::new();
        declarations.insert("m0000:entity_alphatest".into(), json!({}));
        for index in 1..COUNT {
            declarations.insert(format!("m{index:04}:m{:04}", index - 1), json!({}));
        }
        declarations.insert("version".into(), json!("1.0.0"));
        let mut states = MaterialStates::default();
        states.read(&json!({"materials": declarations}));
        let (report, allocations) = test_alloc::measure(|| {
            let mut table = MaterialTable::default();
            let mut provenance = MaterialProvenance::default();
            for index in (0..COUNT).rev() {
                let name = format!("m{index:04}");
                let (_, definition) = states.resolve(&name, &mut table).unwrap();
                states.record_provenance(&name, definition, &mut table, &mut provenance);
            }
            provenance.finish()
        });
        assert_eq!(report.len(), COUNT);
        assert_eq!(report.last().unwrap().chain.iter().count(), COUNT + 1);
        assert!(allocations.bytes < COUNT * 8192, "{allocations:?}");
    }

    #[test]
    fn provenance_follows_a_builtin_base_overridden_in_another_file() {
        let mut states = MaterialStates::default();
        states.read_with(
            &json!({"materials": {"version": "1.0.0",
            "child:entity_alphatest": {}}}),
            &Default::default(),
            "materials/child.material",
        );
        states.read_with(
            &json!({"materials": {"version": "1.0.0",
            "entity_alphatest:entity": {"+defines": ["USE_EMISSIVE"]}}}),
            &Default::default(),
            "materials/base.material",
        );
        let mut table = MaterialTable::default();
        let mut provenance = MaterialProvenance::default();
        for name in ["child", "entity_alphatest"] {
            let (_, definition) = states.resolve(name, &mut table).unwrap();
            states.record_provenance(name, definition, &mut table, &mut provenance);
        }
        let report = provenance.finish();
        assert_eq!(report[0].source.as_ref(), "materials/child.material");
        assert_eq!(
            report[0].chain.iter().collect::<Vec<_>>(),
            ["child", "entity_alphatest", "entity (client base)"]
        );
        assert_eq!(report[1].source.as_ref(), "materials/base.material");
        assert_eq!(
            report[1].chain.iter().collect::<Vec<_>>(),
            ["entity_alphatest", "entity (client base)"]
        );
    }
}
