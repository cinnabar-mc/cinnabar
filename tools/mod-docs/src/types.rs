//! WIT spellings and member documentation for the resolved type graph.

use crate::render::Renderer;
use anyhow::{Result, bail};
use wit_parser::{Docs, Handle, Type, TypeDefKind};

/// Escapes identifiers that WIT otherwise treats as grammar keywords.
pub(super) fn ident(name: &str) -> String {
    match name {
        "use" | "type" | "func" | "record" | "variant" | "enum" | "flags" | "resource"
        | "world" | "interface" | "import" | "export" | "package" | "include" | "with" | "as"
        | "constructor" | "static" | "borrow" | "own" | "bool" | "string" | "char" | "list"
        | "option" | "result" | "tuple" | "future" | "stream" | "map" => format!("%{name}"),
        _ => name.into(),
    }
}

type Members<'a> = Vec<(String, String, &'a Docs)>;

impl Renderer<'_> {
    /// Spells named references or recursively expands anonymous WIT containers.
    pub fn ty(&self, ty: Type) -> Result<String> {
        Ok(match ty {
            Type::Id(id) => {
                let ty = &self.resolve.types[id];
                if let Some(name) = &ty.name {
                    ident(name)
                } else {
                    self.container(&ty.kind)?
                }
            }
            Type::Bool => "bool".into(),
            Type::U8 => "u8".into(),
            Type::U16 => "u16".into(),
            Type::U32 => "u32".into(),
            Type::U64 => "u64".into(),
            Type::S8 => "s8".into(),
            Type::S16 => "s16".into(),
            Type::S32 => "s32".into(),
            Type::S64 => "s64".into(),
            Type::F32 => "f32".into(),
            Type::F64 => "f64".into(),
            Type::Char => "char".into(),
            Type::String => "string".into(),
            Type::ErrorContext => "error-context".into(),
        })
    }

    /// Renders generic containers without dropping optional result arms or tuple elements.
    fn container(&self, kind: &TypeDefKind) -> Result<String> {
        Ok(match kind {
            TypeDefKind::Type(ty) => self.ty(*ty)?,
            TypeDefKind::List(ty) => format!("list<{}>", self.ty(*ty)?),
            TypeDefKind::FixedLengthList(ty, size) => format!("list<{}, {size}>", self.ty(*ty)?),
            TypeDefKind::Option(ty) => format!("option<{}>", self.ty(*ty)?),
            TypeDefKind::Tuple(tuple) => format!(
                "tuple<{}>",
                tuple
                    .types
                    .iter()
                    .map(|ty| self.ty(*ty))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ")
            ),
            TypeDefKind::Map(key, value) => {
                format!("map<{}, {}>", self.ty(*key)?, self.ty(*value)?)
            }
            TypeDefKind::Result(result) => match (result.ok, result.err) {
                (None, None) => "result".into(),
                (Some(ok), None) => format!("result<{}>", self.ty(ok)?),
                (ok, Some(err)) => format!(
                    "result<{}, {}>",
                    ok.map(|ty| self.ty(ty))
                        .transpose()?
                        .unwrap_or_else(|| "_".into()),
                    self.ty(err)?
                ),
            },
            TypeDefKind::Future(ty) | TypeDefKind::Stream(ty) => match ty {
                Some(ty) => format!("{}<{}>", kind.as_str(), self.ty(*ty)?),
                None => kind.as_str().into(),
            },
            TypeDefKind::Handle(Handle::Own(id)) => format!("own<{}>", self.ty(Type::Id(*id))?),
            TypeDefKind::Handle(Handle::Borrow(id)) => {
                format!("borrow<{}>", self.ty(Type::Id(*id))?)
            }
            _ => bail!("unsupported anonymous WIT type: {}", kind.as_str()),
        })
    }

    /// Produces a WIT declaration and the original documentation for its members.
    pub fn definition<'a>(
        &self,
        name: &str,
        kind: &'a TypeDefKind,
    ) -> Result<(String, Members<'a>)> {
        let mut members = Vec::new();
        match kind {
            TypeDefKind::Record(record) => {
                for field in &record.fields {
                    members.push((field.name.clone(), self.ty(field.ty)?, &field.docs));
                }
            }
            TypeDefKind::Variant(variant) => {
                for case in &variant.cases {
                    members.push((
                        case.name.clone(),
                        case.ty
                            .map(|ty| self.ty(ty))
                            .transpose()?
                            .unwrap_or_default(),
                        &case.docs,
                    ));
                }
            }
            TypeDefKind::Enum(value) => {
                for case in &value.cases {
                    members.push((case.name.clone(), String::new(), &case.docs));
                }
            }
            TypeDefKind::Flags(flags) => {
                for flag in &flags.flags {
                    members.push((flag.name.clone(), String::new(), &flag.docs));
                }
            }
            TypeDefKind::Resource => bail!("resource methods need documentation support: {name}"),
            _ => {
                return Ok((
                    format!("type {} = {};", ident(name), self.container(kind)?),
                    members,
                ));
            }
        }
        let fields = members
            .iter()
            .map(|(name, ty, _)| match kind {
                TypeDefKind::Record(_) => format!("    {}: {ty},", ident(name)),
                _ if !ty.is_empty() => format!("    {}({ty}),", ident(name)),
                _ => format!("    {},", ident(name)),
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok((
            format!("{} {} {{\n{fields}\n}}", kind.as_str(), ident(name)),
            members,
        ))
    }

    /// Exposes direct child types for cross-links, stopping recursion at named definitions.
    pub fn member_types(&self, kind: &TypeDefKind) -> Vec<Type> {
        match kind {
            TypeDefKind::Record(record) => record.fields.iter().map(|field| field.ty).collect(),
            TypeDefKind::Variant(variant) => {
                variant.cases.iter().filter_map(|case| case.ty).collect()
            }
            TypeDefKind::Tuple(tuple) => tuple.types.clone(),
            TypeDefKind::Type(ty)
            | TypeDefKind::List(ty)
            | TypeDefKind::Option(ty)
            | TypeDefKind::FixedLengthList(ty, _) => vec![*ty],
            TypeDefKind::Result(result) => result.ok.into_iter().chain(result.err).collect(),
            TypeDefKind::Map(key, value) => vec![*key, *value],
            TypeDefKind::Future(ty) | TypeDefKind::Stream(ty) => ty.iter().copied().collect(),
            TypeDefKind::Handle(Handle::Own(id) | Handle::Borrow(id)) => vec![Type::Id(*id)],
            _ => Vec::new(),
        }
    }
}
