use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use proc_macro2::TokenTree;
use syn::visit::Visit;

use crate::{
    ArchitectureError,
    paths::relative_slash,
    policy::Policy,
    read,
    rust_scope::{is_rust_test, marks_test_scope},
};

/// What a system reads keyboard, pointer, touch or gamepad input through.
const INPUT_TYPES: [&str; 8] = [
    "ButtonInput",
    "Gamepad",
    "KeyboardInput",
    "MouseButtonInput",
    "MouseWheel",
    "TouchInput",
    "Touches",
    "WindowEvent",
];
const OWNER: &str = "InputOwner";
const HELP: &str = "a screen takes input as the frame's owner through `InputOwner`; anything else needs an `[input_owner]` exception saying why it isn't a screen";

/// Requires every system under the configured root that takes input to take [`OWNER`] too, so a
/// screen can't read input without being the frame's owner. A helper handed input by reference,
/// optional reference or iterator, or a modifier tracker in a `Local`, has no input of its own.
pub(super) fn check_input_owner(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.input_owner;
    let Some(owner_root) = rules.root.as_deref() else {
        return Ok(());
    };
    let features: BTreeSet<String> = policy
        .crate_rules
        .iter()
        .flat_map(|rule| rule.test_support_features.iter().cloned())
        .collect();
    let mut readers = Vec::new();
    for path in files {
        let relative = relative_slash(root, path);
        if !relative.starts_with(owner_root)
            || !relative.ends_with(".rs")
            || is_rust_test(&relative)
        {
            continue;
        }
        let file = match syn::parse_file(&read(path)?) {
            Ok(file) => file,
            Err(error) => {
                diagnostics.push(format!(
                    "{relative}:{}: cannot parse Rust for input owner check: {error}",
                    error.span().start().line
                ));
                continue;
            }
        };
        let mut scan = Scan {
            features: &features,
            readers: Vec::new(),
        };
        scan.visit_file(&file);
        readers.extend(
            scan.readers
                .into_iter()
                .map(|(name, line)| (format!("{relative}::{name}"), relative.clone(), line)),
        );
    }
    for (item, relative, line) in &readers {
        if !rules.exceptions.iter().any(|known| known.item == *item) {
            diagnostics.push(format!("{relative}:{line}: {item}: {HELP}"));
        }
    }
    for exception in &rules.exceptions {
        if exception.reason.trim().is_empty() {
            diagnostics.push(format!(
                "{}: input owner exception needs a reason",
                exception.item
            ));
        }
        if !readers.iter().any(|(item, ..)| *item == exception.item) {
            diagnostics.push(format!(
                "{}: stale input owner exception; it no longer reads input without the owner",
                exception.item
            ));
        }
    }
    Ok(())
}

/// Collects the name and line of each item that takes input without the owner.
struct Scan<'a> {
    features: &'a BTreeSet<String>,
    readers: Vec<(String, usize)>,
}

impl Scan<'_> {
    /// Notes `name` if `types`, a system's parameters or fields, take input without the owner.
    fn check<'t>(
        &mut self,
        name: String,
        span: proc_macro2::Span,
        types: impl Iterator<Item = &'t syn::Type>,
    ) {
        let (mut input, mut owner) = (false, false);
        for ty in types {
            let (takes_input, takes_owner) = reads(ty);
            input |= takes_input;
            owner |= takes_owner;
        }
        if input && !owner {
            self.readers.push((name, span.start().line));
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !marks_test_scope(&item.attrs, self.features) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !marks_test_scope(&item.attrs, self.features) {
            self.check_signature(&item.sig);
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !marks_test_scope(&item.attrs, self.features) {
            self.check_signature(&item.sig);
            syn::visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if !marks_test_scope(&item.attrs, self.features) && derives_system_param(&item.attrs) {
            self.check(
                item.ident.to_string(),
                item.ident.span(),
                item.fields.iter().map(|field| &field.ty),
            );
        }
    }
}

impl Scan<'_> {
    fn check_signature(&mut self, signature: &syn::Signature) {
        self.check(
            signature.ident.to_string(),
            signature.ident.span(),
            signature.inputs.iter().filter_map(|input| match input {
                syn::FnArg::Typed(input) => Some(&*input.ty),
                syn::FnArg::Receiver(_) => None,
            }),
        );
    }
}

fn derives_system_param(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("derive")
            && attribute.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .clone()
                    .into_iter()
                    .any(|token| matches!(token, TokenTree::Ident(ident) if ident == "SystemParam"))
            })
    })
}

/// Whether a system parameter of type `ty` takes input, and whether it takes the owner.
fn reads(ty: &syn::Type) -> (bool, bool) {
    match ty {
        syn::Type::Reference(_) | syn::Type::ImplTrait(_) => (false, false),
        syn::Type::Tuple(tuple) => tuple.elems.iter().map(reads).fold(
            (false, false),
            |(input, owner), (takes_input, takes_owner)| {
                (input || takes_input, owner || takes_owner)
            },
        ),
        syn::Type::Path(path) if is_local(path) || is_optional_reference(path) => (false, false),
        ty => {
            let mut names = TypeNames::default();
            names.visit_type(ty);
            (names.input, names.owner)
        }
    }
}

fn is_local(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Local")
}

/// Whether `path` is `Option<&T>`, which a helper takes and a system can't.
fn is_optional_reference(path: &syn::TypePath) -> bool {
    path.path.segments.last().is_some_and(|segment| {
        segment.ident == "Option"
            && matches!(
                &segment.arguments,
                syn::PathArguments::AngleBracketed(arguments)
                    if matches!(
                        arguments.args.first(),
                        Some(syn::GenericArgument::Type(syn::Type::Reference(_)))
                    )
            )
    })
}

/// Which of the names this gate knows a type mentions.
#[derive(Default)]
struct TypeNames {
    input: bool,
    owner: bool,
}

impl Visit<'_> for TypeNames {
    fn visit_ident(&mut self, ident: &proc_macro2::Ident) {
        self.input |= INPUT_TYPES.iter().any(|name| ident == name);
        self.owner |= ident == OWNER;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the gate on `sources` under a temporary repository whose owner root is `src/`.
    fn diagnostics(exceptions: &str, sources: &[(&str, &str)]) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let files: Vec<_> = sources
            .iter()
            .map(|(name, source)| {
                let path = root.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, source).unwrap();
                path
            })
            .collect();
        let policy = toml::from_str::<Policy>(&format!(
            "production_rust_max = 1000\nmodule_root_max = 1000\npowershell_max = 1000\ntest_max = 1000\n[input_owner]\nroot = \"src/\"\n{exceptions}"
        ))
        .unwrap();
        let mut diagnostics = Vec::new();
        check_input_owner(root.path(), &policy, &files, &mut diagnostics).unwrap();
        diagnostics
    }

    #[test]
    fn a_system_reading_input_without_the_owner_fails() {
        for source in [
            "fn drive(keys: MessageReader<KeyboardInput>) {}",
            "fn drive(keys: Res<ButtonInput<KeyCode>>) {}",
            "fn drive(messages: Option<Res<Messages<MouseButtonInput>>>) {}",
            "fn drive(pads: Query<&Gamepad>) {}",
            "fn drive(input: (Res<Touches>, Local<u8>)) {}",
            "#[derive(SystemParam)] struct Drive<'w> { keys: Res<'w, ButtonInput<KeyCode>> }",
        ] {
            let found = diagnostics("", &[("src/screen.rs", source)]);
            assert_eq!(found.len(), 1, "{source}: {found:?}");
            assert!(found[0].starts_with("src/screen.rs:1:"), "{found:?}");
        }
    }

    #[test]
    fn a_system_with_the_owner_and_things_that_read_nothing_pass() {
        for source in [
            "fn drive(keys: MessageReader<KeyboardInput>, owner: InputOwner) {}",
            "fn drive(keys: MessageReader<KeyboardInput>, both: (Res<A>, InputOwner)) {}",
            "#[derive(SystemParam)] struct D<'w> { keys: Res<'w, Touches>, owner: InputOwner<'w> }",
            "fn helper(keys: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>) {}",
            "fn helper<'a>(pads: impl Iterator<Item = &'a Gamepad>) {}",
            "fn helper(messages: Option<&Messages<MouseButtonInput>>) {}",
            "fn track(modifiers: Local<ButtonInput<KeyCode>>) {}",
            "struct Observed { modifiers: ButtonInput<KeyCode> }",
            "#[cfg(test)] mod tests { fn fixture(touches: Res<Touches>) {} }",
        ] {
            let found = diagnostics("", &[("src/screen.rs", source)]);
            assert_eq!(found, Vec::<String>::new(), "{source}");
        }
    }

    #[test]
    fn only_the_owner_root_is_checked() {
        let reader = "fn drive(keys: Res<ButtonInput<KeyCode>>) {}";
        assert!(diagnostics("", &[("other/screen.rs", reader)]).is_empty());
        assert!(diagnostics("", &[("src/screen/tests.rs", reader)]).is_empty());
    }

    #[test]
    fn exceptions_need_a_reason_and_a_reader() {
        let reader = "fn drive(keys: Res<ButtonInput<KeyCode>>) {}";
        let exception = |reason| {
            format!(
                "[[input_owner.exceptions]]\nitem = \"src/screen.rs::drive\"\nreason = \"{reason}\"\n"
            )
        };
        assert!(
            diagnostics(&exception("Gameplay input."), &[("src/screen.rs", reader)]).is_empty()
        );
        let empty = diagnostics(&exception(" "), &[("src/screen.rs", reader)]);
        assert!(empty[0].contains("needs a reason"), "{empty:?}");
        let stale = diagnostics(
            &exception("Gameplay input."),
            &[("src/screen.rs", "fn drive() {}")],
        );
        assert!(stale[0].contains("stale"), "{stale:?}");
    }
}
