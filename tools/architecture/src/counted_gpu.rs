use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use syn::{spanned::Spanned, visit::Visit};

use crate::{
    ArchitectureError,
    paths::relative_slash,
    policy::Policy,
    read,
    rust_scope::{
        ModulePaths, is_rust_test, item_attributes, marks_test_scope, reachable_production_files,
    },
};

/// Each raw GPU request the render-frame counters cannot see, with its counted replacement.
const RAW_CALLS: [(&str, &str); 6] = [
    ("begin_render_pass", "counted_render_pass"),
    ("begin_compute_pass", "counted_compute_pass"),
    ("begin_tracked_render_pass", "counted_tracked_render_pass"),
    ("write_buffer", "profiled_write_buffer"),
    ("write_buffer_with", "profiled_write_buffer_with_at"),
    ("write_texture", "profiled_write_texture"),
];

/// Keeps the render-frame work counters complete. Production code in the covered crates may
/// begin a pass or write to the queue only through the counted entry points, which live in the
/// policy's entry-point files. A call with a raw method's name on some other receiver needs a
/// listed exception with a reason, and an exception nothing uses any more is reported. Test
/// code may call the raw methods.
pub(super) fn check_counted_gpu(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.counted_gpu;
    if rules.crates.is_empty() {
        return Ok(());
    }
    for exception in &rules.exceptions {
        if exception.reason.trim().is_empty() {
            diagnostics.push(format!(
                "{}: raw GPU call exception for `{}` needs a reason",
                exception.path, exception.method
            ));
        }
        if !RAW_CALLS.iter().any(|(raw, _)| *raw == exception.method) {
            diagnostics.push(format!(
                "{}: `{}` is not a counted GPU request; remove its exception",
                exception.path, exception.method
            ));
        }
    }
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
        let mut scan = Scan {
            test_features: &test_features,
            file_test: is_rust_test(&relative),
            in_test: false,
            module_paths: ModulePaths::new(path),
            calls: Vec::new(),
            modules: Vec::new(),
        };
        scan.visit_file(&file);
        scans.insert(
            path.canonicalize().unwrap_or_else(|_| path.clone()),
            (relative, scan),
        );
    }
    for entry_point in &rules.entry_points {
        if !scans.values().any(|(relative, _)| relative == entry_point) {
            diagnostics.push(format!("{entry_point}: counted GPU entry point not found"));
        }
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
    let mut used = vec![false; rules.exceptions.len()];
    for (path, (relative, scan)) in &scans {
        let covered = rules
            .crates
            .iter()
            .any(|krate| relative.starts_with(&format!("{krate}/src/")));
        if !covered
            || scan.file_test
            || !production_files.contains(path)
            || rules.entry_points.contains(relative)
        {
            continue;
        }
        for call in scan.calls.iter().filter(|call| !call.test) {
            let (raw, counted) = RAW_CALLS[call.method];
            if let Some(index) = rules
                .exceptions
                .iter()
                .position(|exception| exception.path == *relative && exception.method == raw)
            {
                used[index] = true;
                continue;
            }
            diagnostics.push(format!(
                "{relative}:{}: `{raw}` is not counted toward the render frame; use `{counted}`, or list an exception if the receiver is not a pass encoder or the queue",
                call.line
            ));
        }
    }
    for (exception, used) in rules.exceptions.iter().zip(used) {
        if !used {
            diagnostics.push(format!(
                "{}: stale raw GPU call exception; production code there no longer calls `{}`",
                exception.path, exception.method
            ));
        }
    }
    Ok(())
}

/// One call of a raw method by name.
struct Call {
    /// Index into [`RAW_CALLS`].
    method: usize,
    line: usize,
    /// Whether a test-only item encloses it.
    test: bool,
}

/// One file's raw calls and the modules it declares, with the test scope of each.
struct Scan<'a> {
    test_features: &'a BTreeSet<String>,
    file_test: bool,
    in_test: bool,
    module_paths: ModulePaths,
    calls: Vec<Call>,
    modules: Vec<(PathBuf, bool)>,
}

impl Scan<'_> {
    /// Records a call when `name` is one of the raw methods.
    fn record(&mut self, name: &str, span: Span) {
        if let Some(method) = RAW_CALLS.iter().position(|(raw, _)| *raw == name) {
            self.calls.push(Call {
                method,
                line: span.start().line,
                test: self.in_test,
            });
        }
    }

    /// Finds `.method(` and `path::method(` in macro tokens, descending into every group.
    fn scan_macro_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            match token {
                TokenTree::Group(group) => self.scan_macro_tokens(group.stream()),
                TokenTree::Ident(name) => {
                    let called = matches!(
                        tokens.get(index + 1),
                        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis
                    );
                    let qualified = index > 0
                        && matches!(
                            &tokens[index - 1],
                            TokenTree::Punct(punct) if matches!(punct.as_char(), '.' | ':')
                        );
                    if called && qualified {
                        self.record(&name.to_string(), name.span());
                    }
                }
                _ => {}
            }
        }
    }

    /// Runs `visit` with test scope widened by `attributes`, then restores it.
    fn scoped(&mut self, attributes: &[syn::Attribute], visit: impl FnOnce(&mut Self)) {
        let previous = self.in_test;
        self.in_test |= marks_test_scope(attributes, self.test_features);
        visit(self);
        self.in_test = previous;
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    /// Applies file-level test attributes before visiting the file's items.
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.file_test |= marks_test_scope(&file.attrs, self.test_features);
        self.in_test = self.file_test;
        syn::visit::visit_file(self, file);
    }

    /// Keeps cfg-based test scope inside the attributed item.
    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.scoped(item_attributes(item), |scan| {
            syn::visit::visit_item(scan, item);
        });
    }

    /// Applies test scope to individual impl members without affecting their siblings.
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

    /// Scans inline modules and records resolved files for out-of-line declarations.
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.content.is_some() {
            let old_paths = self.module_paths.clone();
            self.module_paths.push_inline(&module.ident.to_string());
            syn::visit::visit_item_mod(self, module);
            self.module_paths = old_paths;
        } else if let Some(path) = self.module_paths.resolve(module) {
            self.modules.push((path, self.in_test));
        }
    }

    /// Scans macro arguments, which the parser leaves as tokens.
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.scan_macro_tokens(mac.tokens.clone());
    }

    /// Records `receiver.method(..)`.
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.record(&call.method.to_string(), call.method.span());
        syn::visit::visit_expr_method_call(self, call);
    }

    /// Records `Type::method(receiver, ..)`.
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func
            && function.path.segments.len() > 1
            && let Some(last) = function.path.segments.last()
        {
            self.record(&last.ident.to_string(), call.span());
        }
        syn::visit::visit_expr_call(self, call);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "crates/a/src/counting.rs";
    const LIB: &str = "mod counting;\nmod draw;\n#[cfg(test)]\nmod fixture;\n";
    const COUNTING: &str =
        "fn counted_render_pass(encoder: &mut Encoder) { encoder.begin_render_pass(&desc()); }\n";

    /// Runs the check on a temporary repository whose crate `a` is covered, with `exceptions`
    /// appended to the policy.
    fn diagnostics(sources: &[(&str, &str)], exceptions: &str) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let files: Vec<_> = sources
            .iter()
            .copied()
            .chain([("crates/a/src/lib.rs", LIB), (ENTRY, COUNTING)])
            .map(|(name, source)| {
                let path = root.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, source).unwrap();
                path
            })
            .collect();
        let policy = toml::from_str::<Policy>(&format!(
            "production_rust_max = 1000\nmodule_root_max = 1000\npowershell_max = 1000\ntest_max = 1000\n[[crates]]\nname = 'a'\npath = 'crates/a'\ntest_support_features = ['test-support']\n[counted_gpu]\ncrates = ['crates/a']\nentry_points = ['{ENTRY}']\n{exceptions}"
        ))
        .unwrap();
        let mut diagnostics = Vec::new();
        check_counted_gpu(root.path(), &policy, &files, &mut diagnostics).unwrap();
        diagnostics.sort();
        diagnostics
    }

    #[test]
    fn counted_requests_and_the_entry_point_pass() {
        let found = diagnostics(
            &[(
                "crates/a/src/draw.rs",
                "fn draw(encoder: &mut Encoder, queue: &Queue) {\n    encoder.counted_render_pass(&desc());\n    queue.profiled_write_buffer(&buffer, 0, &[]);\n}\n",
            )],
            "",
        );
        assert_eq!(found, [] as [String; 0]);
    }

    #[test]
    fn raw_passes_and_queue_writes_in_production_code_are_rejected() {
        let found = diagnostics(
            &[(
                "crates/a/src/draw.rs",
                "fn draw(encoder: &mut Encoder, context: &mut Context, queue: &Queue) {\n    encoder.begin_render_pass(&desc());\n    encoder.begin_compute_pass(&desc());\n    context.begin_tracked_render_pass(desc());\n    queue.write_buffer(&buffer, 0, &[]);\n    queue.write_texture(texture, &[], layout, size);\n    wgpu::Queue::write_buffer(queue, &buffer, 0, &[]);\n    assert!(queue.write_buffer_with(&buffer, 0, size).is_some());\n}\n",
            )],
            "",
        );
        let lines: Vec<_> = found
            .iter()
            .map(|line| line.split(": ").next().unwrap())
            .collect();
        assert_eq!(
            lines,
            (2..=8)
                .map(|line| format!("crates/a/src/draw.rs:{line}"))
                .collect::<Vec<_>>(),
            "{found:?}"
        );
        assert!(found[0].contains("use `counted_render_pass`"), "{found:?}");
        assert!(
            found[3].contains("use `profiled_write_buffer`"),
            "{found:?}"
        );
    }

    #[test]
    fn test_code_may_call_the_raw_methods() {
        let found = diagnostics(
            &[
                (
                    "crates/a/src/draw.rs",
                    "#[cfg(test)]\nmod tests {\n    fn raw(queue: &Queue) { queue.write_buffer(&buffer, 0, &[]); }\n}\n#[cfg(any(test, feature = \"test-support\"))]\nfn fixture(encoder: &mut Encoder) { encoder.begin_render_pass(&desc()); }\n",
                ),
                (
                    "crates/a/src/fixture.rs",
                    "fn raw(encoder: &mut Encoder) { encoder.begin_compute_pass(&desc()); }\n",
                ),
                (
                    "crates/a/src/draw_tests.rs",
                    "fn raw(queue: &Queue) { queue.write_texture(texture, &[], layout, size); }\n",
                ),
            ],
            "",
        );
        assert_eq!(found, [] as [String; 0]);
    }

    /// An integration test including a production file by path leaves it production code.
    #[test]
    fn production_files_included_by_tests_stay_covered() {
        let found = diagnostics(
            &[
                (
                    "crates/a/src/draw.rs",
                    "fn draw(queue: &Queue) { queue.write_buffer(&buffer, 0, &[]); }\n",
                ),
                (
                    "crates/a/tests/it/main.rs",
                    "#[path = \"../../src/draw.rs\"]\nmod draw;\n",
                ),
            ],
            "",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("crates/a/src/draw.rs:1: "),
            "{found:?}"
        );
    }

    #[test]
    fn other_crates_are_not_covered() {
        let found = diagnostics(
            &[(
                "crates/b/src/lib.rs",
                "fn draw(queue: &Queue) { queue.write_buffer(&buffer, 0, &[]); }\n",
            )],
            "",
        );
        assert_eq!(found, [] as [String; 0]);
    }

    #[test]
    fn exceptions_cover_one_method_in_one_file_and_must_stay_used() {
        let belt = "fn stage(belt: &mut Belt) { belt.write_buffer(encoder, &buffer, 0, size); }\n";
        let exception = |path: &str, reason: &str| {
            format!(
                "[[counted_gpu.exceptions]]\npath = '{path}'\nmethod = 'write_buffer'\nreason = '{reason}'\n"
            )
        };
        let allowed = diagnostics(
            &[("crates/a/src/draw.rs", belt)],
            &exception("crates/a/src/draw.rs", "a staging belt, not the queue"),
        );
        assert_eq!(allowed, [] as [String; 0]);
        let other_method = diagnostics(
            &[(
                "crates/a/src/draw.rs",
                "fn draw(queue: &Queue) { queue.write_texture(texture, &[], layout, size); }\n",
            )],
            &exception("crates/a/src/draw.rs", "a staging belt, not the queue"),
        );
        assert_eq!(other_method.len(), 2, "{other_method:?}");
        for expected in [
            "`write_texture` is not counted",
            "stale raw GPU call exception",
        ] {
            assert!(
                other_method.iter().any(|line| line.contains(expected)),
                "{other_method:?}"
            );
        }
        let unexplained = diagnostics(
            &[("crates/a/src/draw.rs", belt)],
            &exception("crates/a/src/draw.rs", " "),
        );
        assert_eq!(unexplained.len(), 1, "{unexplained:?}");
        assert!(unexplained[0].contains("needs a reason"));
    }

    #[test]
    fn a_missing_entry_point_is_reported() {
        let root = tempfile::tempdir().unwrap();
        let policy = toml::from_str::<Policy>(
            "production_rust_max = 1000\nmodule_root_max = 1000\npowershell_max = 1000\ntest_max = 1000\n[counted_gpu]\ncrates = ['crates/a']\nentry_points = ['crates/a/src/gone.rs']",
        )
        .unwrap();
        let mut found = Vec::new();
        check_counted_gpu(root.path(), &policy, &[], &mut found).unwrap();
        assert_eq!(
            found,
            ["crates/a/src/gone.rs: counted GPU entry point not found"]
        );
    }
}
