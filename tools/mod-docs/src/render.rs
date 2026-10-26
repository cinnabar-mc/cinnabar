//! Interface, world and callback pages from the resolved contract.

use crate::{HEADER, PRIMARY_PACKAGE, anchor, cell, docs, imported, owner_anchor};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use wit_parser::{
    Docs, Function, InterfaceId, Resolve, Type, TypeDefKind, TypeId, WorldItem, WorldKey,
};

// Only names whose spelling or wording does not follow the kebab-case derivation.
pub(super) const TITLE_OVERRIDES: &[(&str, &str)] = &[
    ("world-draw", "world drawing"),
    ("hud", "HUD"),
    ("ui", "UI"),
];

/// Turns a WIT name into words, preserving established acronyms and natural wording.
fn readable_name(name: &str) -> String {
    TITLE_OVERRIDES
        .iter()
        .find_map(|(source, title)| (*source == name).then(|| (*title).to_owned()))
        .unwrap_or_else(|| name.replace('-', " "))
}

pub(super) struct Renderer<'a> {
    pub resolve: &'a Resolve,
    pub interfaces: &'a BTreeMap<String, InterfaceId>,
}

impl Renderer<'_> {
    /// Names the versioned package as well as its interface, including repeated short names.
    pub fn interface_name(&self, id: InterfaceId) -> String {
        self.resolve.id_of(id).expect("named package interface")
    }

    /// Derives a readable title and qualifies dependencies by their owning package.
    pub fn interface_title(&self, id: InterfaceId) -> String {
        let interface = &self.resolve.interfaces[id];
        let name = readable_name(interface.name.as_deref().expect("named interface"));
        let package = &self.resolve.packages[interface.package.expect("interface package")];
        let mut title = if package.name.name == PRIMARY_PACKAGE {
            name
        } else {
            format!("{}: {name}", readable_name(&package.name.name))
        };
        title[..1].make_ascii_uppercase();
        title
    }

    /// Finds the generated file for an interface already admitted to the page set.
    pub fn file(&self, id: InterfaceId) -> &str {
        self.interfaces
            .iter()
            .find(|(_, candidate)| **candidate == id)
            .unwrap()
            .0
    }

    /// Requires grant documentation beside its owning WIT interface.
    fn grants<'a>(&self, source: &'a Docs, context: &str) -> Result<(&'a str, String)> {
        let text = docs(source, context)?;
        let grants = text
            .lines()
            .find_map(|line| line.strip_prefix("Grants: "))
            .with_context(|| format!("missing Grants: documentation: {context}"))?;
        let prose = text
            .lines()
            .filter(|line| !line.starts_with("Grants: "))
            .collect::<Vec<_>>()
            .join("\n");
        Ok((grants, prose))
    }

    /// Renders an interface's own types, imported aliases, and callable functions.
    pub fn interface(&self, id: InterfaceId) -> Result<String> {
        let interface = &self.resolve.interfaces[id];
        let name = self.interface_name(id);
        let title = self.interface_title(id);
        let (grants, prose) = self.grants(&interface.docs, &name)?;
        let mut page =
            format!("{HEADER}# {title}\n\n`{name}`\n\n{prose}\n\n**Grants:** {grants}\n\n");
        for (name, ty) in &interface.types {
            if imported(self.resolve, *ty) {
                page.push_str(&format!(
                    "Imported type `{name}`: {}.\n\n",
                    self.type_link(*ty)?
                ));
            }
        }
        page.push_str("## Types\n\n");
        for (name, ty) in &interface.types {
            if !imported(self.resolve, *ty) {
                self.declaration(&mut page, name, *ty)?;
            }
        }
        page.push_str("## Functions\n\n");
        for function in interface.functions.values() {
            self.function(&mut page, function, "")?;
        }
        if interface.functions.is_empty() {
            page.push_str("This interface defines shared types and has no functions.\n");
        }
        Ok(page)
    }

    /// Documents world imports and guest exports, including types declared directly in worlds.
    pub fn worlds(&self) -> Result<String> {
        let mut page = format!(
            "{HEADER}# Worlds and callbacks\n\nA world names the host imports and guest exports of a component. Included worlds are expanded below. Each imported interface documents its own grants.\n\n"
        );
        let mut worlds: Vec<_> = self.resolve.worlds.iter().collect();
        worlds.sort_by_key(|(_, world)| &world.name);
        for (_, world) in worlds {
            page.push_str(&format!(
                "<a id=\"{}\"></a>\n\n## {}\n\n{}\n\n",
                anchor(&world.name),
                world.name,
                docs(&world.docs, &world.name)?
            ));
            for (label, items) in [("Imports", &world.imports), ("Exports", &world.exports)] {
                page.push_str(&format!("### {label}\n\n"));
                for (key, item) in items {
                    match item {
                        WorldItem::Interface { id, .. } => page.push_str(&format!(
                            "- [{}]({}.md)\n",
                            self.interface_name(*id),
                            self.file(*id)
                        )),
                        WorldItem::Type { id, .. } if !imported(self.resolve, *id) => {
                            let WorldKey::Name(name) = key else {
                                anyhow::bail!("unnamed world type")
                            };
                            self.declaration(&mut page, name, *id)?;
                        }
                        WorldItem::Function(function) => {
                            self.function(&mut page, function, &format!("{}-", world.name))?
                        }
                        WorldItem::Type { .. } => {}
                    }
                }
                page.push('\n');
            }
        }
        Ok(page)
    }

    /// Emits an exact WIT signature and links every named parameter and result type.
    fn function(&self, page: &mut String, function: &Function, prefix: &str) -> Result<()> {
        let comment = docs(&function.docs, &function.name)?;
        let params = function
            .params
            .iter()
            .map(|param| {
                Ok(format!(
                    "{}: {}",
                    super::types::ident(&param.name),
                    self.ty(param.ty)?
                ))
            })
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        let result = function
            .result
            .map(|ty| self.ty(ty).map(|name| format!(" -> {name}")))
            .transpose()?
            .unwrap_or_default();
        page.push_str(&format!("<a id=\"{prefix}{}\"></a>\n\n### `{}`\n\n```wit\n{}: func({params}){result};\n```\n\n{comment}\n\n", anchor(&function.name), function.name, super::types::ident(&function.name)));
        let types = function
            .params
            .iter()
            .map(|param| param.ty)
            .chain(function.result);
        self.links(page, types)?;
        Ok(())
    }

    /// Publishes a documented type with field, variant, enum or flag comments preserved.
    fn declaration(&self, page: &mut String, name: &str, id: TypeId) -> Result<()> {
        let ty = &self.resolve.types[id];
        let comment = docs(&ty.docs, name)?;
        page.push_str(&format!(
            "<a id=\"{}\"></a>\n\n### `{name}`\n\n{comment}\n\n",
            owner_anchor(self.resolve, ty.owner, name)
        ));
        let (declaration, members) = self.definition(name, &ty.kind)?;
        page.push_str(&format!("```wit\n{declaration}\n```\n\n"));
        if !members.is_empty() {
            page.push_str("| Member | Type | Description |\n| --- | --- | --- |\n");
            for (name, ty, docs) in members {
                page.push_str(&format!(
                    "| `{name}` | {} | {} |\n",
                    if ty.is_empty() {
                        String::new()
                    } else {
                        format!("`{}`", cell(&ty))
                    },
                    cell(docs.contents.as_deref().unwrap_or("").trim())
                ));
            }
            page.push('\n');
        }
        self.links(page, self.member_types(&ty.kind))?;
        Ok(())
    }

    /// Adds cross-page links outside WIT code blocks without changing the signatures.
    fn links(&self, page: &mut String, types: impl IntoIterator<Item = Type>) -> Result<()> {
        let mut links = BTreeSet::new();
        for ty in types {
            self.collect_links(ty, &mut links)?;
        }
        if !links.is_empty() {
            page.push_str(&format!(
                "Types: {}.\n\n",
                links.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        Ok(())
    }

    /// Follows imported aliases to the page that owns their definition.
    pub fn type_link(&self, mut id: TypeId) -> Result<String> {
        while imported(self.resolve, id) {
            let TypeDefKind::Type(Type::Id(target)) = self.resolve.types[id].kind else {
                unreachable!()
            };
            id = target;
        }
        let ty = &self.resolve.types[id];
        let name = ty.name.as_deref().context("unnamed type link")?;
        let file = match ty.owner {
            wit_parser::TypeOwner::Interface(id) => self.file(id),
            wit_parser::TypeOwner::World(_) => "worlds",
            wit_parser::TypeOwner::None => anyhow::bail!("named type without owner: {name}"),
        };
        Ok(format!(
            "[`{name}`]({file}.md#{})",
            owner_anchor(self.resolve, ty.owner, name)
        ))
    }

    /// Walks anonymous containers while keeping named types as links to their definitions.
    fn collect_links(&self, ty: Type, links: &mut BTreeSet<String>) -> Result<()> {
        if let Type::Id(id) = ty {
            let ty = &self.resolve.types[id];
            if ty.name.is_some() {
                links.insert(self.type_link(id)?);
            } else {
                for member in self.member_types(&ty.kind) {
                    self.collect_links(member, links)?;
                }
            }
        }
        Ok(())
    }
}
