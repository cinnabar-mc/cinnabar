use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use crate::{
    ArchitectureError,
    paths::relative_slash,
    policy::Policy,
    read,
    rust_scope::{
        ModulePaths, is_rust_test, item_attributes, marks_test_scope, reachable_production_files,
    },
};

/// The names production code reaches wgpu's feature constants through.
const FEATURE_TYPES: [&str; 2] = ["Features", "WgpuFeatures"];

/// Keeps the device's feature request and the code that uses features in step. The owner file's
/// constants list every wgpu feature the client requests, so production code elsewhere may name
/// only those: a check for any other feature would read one the device never enabled. Each
/// feature in the owner's `checked` constant must also still be named by production code, so a
/// feature nothing uses any more leaves the request. Test code may name any feature.
pub(super) fn check_device_features(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.device_features;
    let Some(owner) = rules.owner.as_deref() else {
        return Ok(());
    };
    let test_features: BTreeSet<String> = policy
        .crate_rules
        .iter()
        .flat_map(|rule| rule.test_support_features.iter().cloned())
        .collect();
    let mut scans = BTreeMap::new();
    for path in files {
        let relative = relative_slash(root, path);
        if !relative.ends_with(".rs") {
            continue;
        }
        // Unparseable sources are reported by the module checks.
        let Ok(file) = syn::parse_file(&read(path)?) else {
            continue;
        };
        let mut aliases = Aliases::default();
        aliases.visit_file(&file);
        let mut scan = Scan {
            test_features: &test_features,
            types: aliases.0,
            file_test: is_rust_test(&relative) || is_development(&relative),
            in_test: false,
            module_paths: ModulePaths::new(path),
            constant: None,
            names: Vec::new(),
            modules: Vec::new(),
        };
        scan.visit_file(&file);
        scans.insert(
            path.canonicalize().unwrap_or_else(|_| path.clone()),
            (relative, scan),
        );
    }
    // A file is production code when a production file declares it outside test scope. Roots are
    // the files no module declaration names. Integration tests also include some production
    // files by path; that does not make them test code.
    let declared: BTreeSet<&PathBuf> = scans
        .values()
        .flat_map(|(_, scan)| scan.modules.iter().map(|(module, _)| module))
        .collect();
    let production_files = reachable_production_files(
        scans
            .iter()
            .map(|(path, (_, scan))| (path, scan.file_test, scan.modules.as_slice())),
        scans
            .iter()
            .filter(|(path, (_, scan))| !scan.file_test && !declared.contains(path))
            .map(|(path, _)| path.clone()),
    );
    let production =
        |path: &PathBuf, scan: &Scan<'_>| !scan.file_test && production_files.contains(path);
    let Some((owner_path, (_, owner_scan))) =
        scans.iter().find(|(_, (relative, _))| relative == owner)
    else {
        diagnostics.push(format!("{owner}: device feature list not found"));
        return Ok(());
    };
    let owned = |constant: Option<&str>| -> BTreeSet<&str> {
        owner_scan
            .names
            .iter()
            .filter(|name| {
                !name.test && constant.is_none_or(|c| name.constant.as_deref() == Some(c))
            })
            .map(|name| name.feature.as_str())
            .collect()
    };
    let requested = if production(owner_path, owner_scan) {
        owned(None)
    } else {
        BTreeSet::new()
    };
    let mut used = BTreeSet::new();
    for (path, (relative, scan)) in &scans {
        if relative == owner || !production(path, scan) {
            continue;
        }
        for name in scan.names.iter().filter(|name| !name.test) {
            used.insert(name.feature.as_str());
            if !requested.contains(name.feature.as_str()) {
                diagnostics.push(format!(
                    "{relative}:{}: the device does not request wgpu feature `{}`; code may only use features listed in {owner}",
                    name.line, name.feature
                ));
            }
        }
    }
    if let Some(checked) = rules.checked.as_deref() {
        let listed = owned(Some(checked));
        if listed.is_empty() {
            diagnostics.push(format!(
                "{owner}: checked device feature list `{checked}` not found"
            ));
        }
        for feature in listed.difference(&used) {
            diagnostics.push(format!(
                "{owner}: `{checked}` requests wgpu feature `{feature}`, which no production code names; remove it from the request"
            ));
        }
    }
    Ok(())
}

/// Whether `relative` is an example or benchmark, which may name any feature as tests may.
fn is_development(relative: &str) -> bool {
    relative
        .split('/')
        .rev()
        .skip(1)
        .any(|directory| matches!(directory, "examples" | "benches"))
}

/// One feature constant named in a source file.
struct Name {
    feature: String,
    line: usize,
    /// Whether a test-only item encloses it.
    test: bool,
    /// The enclosing `const` item, which groups the owner's lists.
    constant: Option<String>,
}

/// Whether `name` is written like a feature constant, not a method or type.
fn is_constant(name: &str) -> bool {
    name.bytes().any(|byte| byte.is_ascii_uppercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Collects the feature type's names in one file, including `use ... as` renames.
struct Aliases(BTreeSet<String>);

impl Default for Aliases {
    fn default() -> Self {
        Self(
            FEATURE_TYPES
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        )
    }
}

impl<'ast> Visit<'ast> for Aliases {
    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        if FEATURE_TYPES.iter().any(|name| rename.ident == name) {
            self.0.insert(rename.rename.to_string());
        }
    }
}

/// Collects the feature constants a file names, with their test scope and module edges.
struct Scan<'a> {
    test_features: &'a BTreeSet<String>,
    types: BTreeSet<String>,
    file_test: bool,
    in_test: bool,
    module_paths: ModulePaths,
    constant: Option<String>,
    names: Vec<Name>,
    modules: Vec<(PathBuf, bool)>,
}

impl Scan<'_> {
    /// Records `feature` when it follows one of the feature type's names.
    fn record(&mut self, ty: &proc_macro2::Ident, feature: &proc_macro2::Ident) {
        let name = feature.to_string();
        if self.types.contains(&ty.to_string()) && is_constant(&name) {
            self.names.push(Name {
                feature: name,
                line: feature.span().start().line,
                test: self.in_test,
                constant: self.constant.clone(),
            });
        }
    }

    /// Finds `Features::NAME` in macro tokens, which syn leaves unparsed.
    fn scan_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                TokenTree::Group(group) => self.scan_tokens(group.stream()),
                TokenTree::Ident(ty) => {
                    if let [
                        TokenTree::Punct(left),
                        TokenTree::Punct(right),
                        TokenTree::Ident(feature),
                        ..,
                    ] = &tokens[index + 1..]
                        && left.as_char() == ':'
                        && right.as_char() == ':'
                    {
                        self.record(ty, feature);
                    }
                }
                _ => {}
            }
        }
    }

    /// Visits `visit` with `attributes` deciding whether its names are test-only.
    fn scoped(&mut self, attributes: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        let previous = self.in_test;
        self.in_test |= marks_test_scope(attributes, self.test_features);
        visit(self);
        self.in_test = previous;
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.file_test |= marks_test_scope(&file.attrs, self.test_features);
        self.in_test = self.file_test;
        syn::visit::visit_file(self, file);
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.scoped(item_attributes(item), |scan| {
            syn::visit::visit_item(scan, item);
        });
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        let previous = self.constant.replace(item.ident.to_string());
        syn::visit::visit_item_const(self, item);
        self.constant = previous;
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes = match item {
            syn::ImplItem::Const(item) => &item.attrs[..],
            syn::ImplItem::Fn(item) => &item.attrs[..],
            syn::ImplItem::Type(item) => &item.attrs[..],
            syn::ImplItem::Macro(item) => &item.attrs[..],
            _ => &[],
        };
        self.scoped(attributes, |scan| {
            syn::visit::visit_impl_item(scan, item);
        });
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_some() {
            let previous = self.module_paths.clone();
            self.module_paths.push_inline(&module.ident.to_string());
            syn::visit::visit_item_mod(self, module);
            self.module_paths = previous;
        } else if let Some(path) = self.module_paths.resolve(module) {
            self.modules.push((path, self.in_test));
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let mut segments = path.segments.iter().rev();
        if let (Some(feature), Some(ty)) = (segments.next(), segments.next()) {
            self.record(&ty.ident, &feature.ident);
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.scan_tokens(mac.tokens.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "crates/render/src/device_features.rs";
    const LISTS: &str = "pub const CHECKED: Features = Features::TIMESTAMP_QUERY.union(Features::PIPELINE_CACHE);\npub const PACKS: Features = Features::SHADER_F64;\n";

    /// Runs the check on a temporary repository holding `sources` beside the owner's lists.
    fn diagnostics(owner: &str, sources: &[(&str, &str)]) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let files: Vec<_> = sources
            .iter()
            .copied()
            .chain([(OWNER, owner)])
            .map(|(name, source)| {
                let path = root.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, source).unwrap();
                path
            })
            .collect();
        let policy = toml::from_str::<Policy>(&format!(
            "production_rust_max = 1000\nmodule_root_max = 1000\npowershell_max = 1000\ntest_max = 1000\n[device_features]\nowner = '{OWNER}'\nchecked = 'CHECKED'"
        ))
        .unwrap();
        let mut diagnostics = Vec::new();
        check_device_features(root.path(), &policy, &files, &mut diagnostics).unwrap();
        diagnostics.sort();
        diagnostics
    }

    const USES: &str = "fn timing(f: wgpu::Features) -> bool { f.contains(wgpu::Features::TIMESTAMP_QUERY) }\nfn cache(f: WgpuFeatures) -> bool { assert!(f.contains(WgpuFeatures::PIPELINE_CACHE)); true }\n";

    #[test]
    fn code_naming_only_requested_features_passes() {
        assert_eq!(
            diagnostics(LISTS, &[("crates/render/src/gpu.rs", USES)]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn production_code_cannot_name_a_feature_the_device_does_not_request() {
        let ray = "use wgpu::Features as F;\nfn rays(f: F) -> bool {\n    f.contains(F::EXPERIMENTAL_RAY_QUERY) || matches!(f, Features::SHADER_F16)\n}\n";
        assert_eq!(
            diagnostics(
                LISTS,
                &[("crates/render/src/gpu.rs", USES), ("app/src/rays.rs", ray)]
            ),
            [
                format!(
                    "app/src/rays.rs:3: the device does not request wgpu feature `EXPERIMENTAL_RAY_QUERY`; code may only use features listed in {OWNER}"
                ),
                format!(
                    "app/src/rays.rs:3: the device does not request wgpu feature `SHADER_F16`; code may only use features listed in {OWNER}"
                ),
            ]
        );
    }

    #[test]
    fn tests_may_name_any_feature() {
        let inline = "#[cfg(test)]\nmod tests {\n    fn all() -> wgpu::Features { wgpu::Features::SHADER_F16 }\n}\n#[cfg(test)]\nmod fixture;\n";
        let fixture = "fn mesh() -> wgpu::Features { wgpu::Features::EXPERIMENTAL_MESH_SHADER }\n";
        assert_eq!(
            diagnostics(
                LISTS,
                &[
                    ("crates/render/src/gpu.rs", USES),
                    ("crates/render/src/lib.rs", inline),
                    ("crates/render/src/fixture.rs", fixture),
                    ("crates/render/src/gpu/tests.rs", fixture),
                ]
            ),
            Vec::<String>::new()
        );
    }

    /// Integration tests include some production files by path.
    #[test]
    fn a_production_file_a_test_crate_also_includes_still_counts() {
        let lib = "mod timing;\nmod cache;\n";
        let timing = "fn timing(f: wgpu::Features) -> bool { f.contains(wgpu::Features::TIMESTAMP_QUERY) }\n";
        let cache = "fn cache(f: wgpu::Features) -> bool { f.contains(wgpu::Features::PIPELINE_CACHE | wgpu::Features::SUBGROUP) }\n";
        let included = "#[path = \"../../src/cache.rs\"]\nmod cache;\n";
        assert_eq!(
            diagnostics(
                LISTS,
                &[
                    ("crates/render/src/lib.rs", lib),
                    ("crates/render/src/timing.rs", timing),
                    ("crates/render/src/cache.rs", cache),
                    ("crates/render/tests/it/main.rs", included),
                    (
                        "crates/render/benches/all.rs",
                        "fn all() -> wgpu::Features { wgpu::Features::SHADER_F16 }\n"
                    ),
                ]
            ),
            [format!(
                "crates/render/src/cache.rs:1: the device does not request wgpu feature `SUBGROUP`; code may only use features listed in {OWNER}"
            )]
        );
    }

    #[test]
    fn a_checked_feature_nothing_names_is_stale() {
        let timing_only = "fn timing(f: wgpu::Features) -> bool { f.contains(wgpu::Features::TIMESTAMP_QUERY) }\n";
        assert_eq!(
            diagnostics(LISTS, &[("crates/render/src/gpu.rs", timing_only)]),
            [format!(
                "{OWNER}: `CHECKED` requests wgpu feature `PIPELINE_CACHE`, which no production code names; remove it from the request"
            )]
        );
    }

    #[test]
    fn a_missing_checked_list_is_reported() {
        assert_eq!(
            diagnostics("pub const PACKS: Features = Features::SHADER_F64;\n", &[]),
            [format!(
                "{OWNER}: checked device feature list `CHECKED` not found"
            )]
        );
    }
}
