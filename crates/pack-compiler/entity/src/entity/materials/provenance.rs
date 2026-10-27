//! Where each material an actor draws came from and how it draws, for reports and inspectors.

use std::collections::BTreeMap;

use assets::{
    entity::material::actor_define_bit, entity_material_report::EntityMaterialProvenance,
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
pub(crate) struct MaterialProvenance(BTreeMap<Box<str>, EntityMaterialProvenance>);

impl MaterialProvenance {
    /// The recorded materials, sorted by name.
    pub(crate) fn finish(self) -> Box<[EntityMaterialProvenance]> {
        self.0.into_values().collect()
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
        if provenance.0.contains_key(target) {
            return;
        }
        let name = material_name(target);
        let Some(resolved) = self.resolve_with(name, &builtin, &mut table.chains) else {
            return;
        };
        let (source, chain) = self.chain(name);
        provenance.0.insert(
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
        if provenance.0.contains_key(name) {
            return;
        }
        let Some((material, resolved)) = self.extension_material(name, &mut table.chains) else {
            return;
        };
        let mut chain = vec![Box::<str>::from(name)];
        if let Some(base) = &material.base {
            chain.extend(self.chain(material_name(base)).1);
        }
        provenance.0.insert(
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

    /// The file declaring `name` and its chain of parents, ending at a client-declared base.
    fn chain(&self, name: &str) -> (Box<str>, Vec<Box<str>>) {
        let mut chain = Vec::new();
        let mut source = None;
        let mut file = None;
        let mut current = Some(name.to_owned());
        // Callers pass resolved materials, whose chains are acyclic; a repeated link still ends
        // the walk.
        let mut visited = std::collections::HashSet::new();
        while let Some(name) = current.take() {
            if !visited.insert((name.clone(), file)) {
                break;
            }
            match self
                .definitions
                .get(name.as_str())
                .filter(|declaration| file.is_none_or(|file| declaration.file == file))
            {
                Some(declaration) => {
                    source.get_or_insert_with(|| {
                        self.file_paths
                            .get(declaration.file)
                            .cloned()
                            .unwrap_or_default()
                    });
                    file = Some(declaration.file);
                    current = declaration.parent.as_deref().map(str::to_owned);
                    chain.push(name.into());
                }
                None if builtin(&name).is_some() => {
                    source.get_or_insert_with(|| CLIENT_BASE.into());
                    chain.push(format!("{name} ({CLIENT_BASE})").into());
                }
                None => chain.push(name.into()),
            }
        }
        (source.unwrap_or_else(|| CLIENT_BASE.into()), chain)
    }
}

/// The provenance of a resolved material as an actor draws it.
fn describe(
    name: &str,
    definition: u32,
    source: Box<str>,
    chain: Vec<Box<str>>,
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
        chain: chain.into(),
        variant: variant.into(),
        applied_defines: applied.into(),
        carried_defines: carried.into(),
        reason: (!reasons.is_empty()).then(|| reasons.join("; ").into()),
    }
}
