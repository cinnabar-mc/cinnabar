//! Deterministic instruction budgets: Callgrind counts each budget test's counted region and the
//! result is checked against `instruction-budgets.json` in both directions.

use crate::DevtoolError;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Committed budgets, relative to the workspace root.
const BUDGET_FILE: &str = "tools/devtool/instruction-budgets.json";
/// Largest change, either way, that keeps a budget current.
const TOLERANCE_PERCENT: f64 = 1.0;
/// Pins glibc's CPU-dependent copy strategies so runners with different CPUs count the same
/// instructions: `rep movsb`/`stosb` retire as one instruction and non-temporal copies switch
/// on cache sizes.
const GLIBC_TUNABLES: &str = "glibc.cpu.x86_rep_movsb_threshold=268435456:\
glibc.cpu.x86_rep_stosb_threshold=268435456:\
glibc.cpu.x86_non_temporal_threshold=268435456";
const UPDATE_COMMAND: &str = "cargo run -p devtool --locked -- instruction-budgets --update";

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Budgets {
    /// `rustc -V` of the toolchain that measured the budgets.
    toolchain: String,
    /// Instructions per `<nextest binary id> <test name>`.
    budgets: BTreeMap<String, u64>,
}

/// One discovered budget test and where nextest would run it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BudgetTest {
    id: String,
    name: String,
    binary: PathBuf,
    cwd: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Within,
    Over,
    Under,
    Unbudgeted,
    Stale,
}

impl Status {
    /// Short table wording for the status.
    fn label(self) -> &'static str {
        match self {
            Self::Within => "ok",
            Self::Over => "over budget",
            Self::Under => "under budget: lower it to keep the gain",
            Self::Unbudgeted => "no budget",
            Self::Stale => "no such test",
        }
    }
}

#[derive(Debug, PartialEq)]
struct Row {
    id: String,
    budget: Option<u64>,
    measured: Option<u64>,
    status: Status,
}

#[derive(Debug)]
struct Options {
    update: bool,
    out: Option<PathBuf>,
}

/// Measures every `instruction_budget_*` test and checks or rewrites the committed budgets.
pub(crate) fn run(args: &[String]) -> Result<(), DevtoolError> {
    let options = parse(args)?;
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(error(
            "budgets are measured on x86-64 Linux; run this through offload or CI",
        ));
    }
    let root = workspace_root()?;
    let scratch = tempfile::tempdir().map_err(|source| spawn_error("create scratch", source))?;
    let out = options.out.clone().unwrap_or_else(|| scratch.path().into());
    fs::create_dir_all(&out).map_err(|source| spawn_error("create output directory", source))?;
    let tests = discover(&root)?;
    let mut measured = Budgets {
        toolchain: toolchain(&root)?,
        budgets: BTreeMap::new(),
    };
    for (index, test) in tests.iter().enumerate() {
        println!("measuring {} ({}/{})", test.id, index + 1, tests.len());
        let profile = out.join(format!("{}.callgrind", file_stem(&test.id)));
        measured
            .budgets
            .insert(test.id.clone(), measure(test, &profile)?);
    }
    write_budgets(&out.join("instruction-budgets.json"), &measured)?;
    let path = root.join(BUDGET_FILE);
    if options.update {
        write_budgets(&path, &measured)?;
        println!("wrote {} budgets to {BUDGET_FILE}", measured.budgets.len());
        return Ok(());
    }
    let committed: Budgets = match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Budgets::default(),
        Err(source) => return Err(spawn_error(&format!("read {BUDGET_FILE}"), source)),
    };
    let rows = compare(&committed, &measured);
    let report = report(&committed, &measured, &rows);
    println!("{report}");
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text = fs::read_to_string(&summary).unwrap_or_default();
        text.push_str(&report);
        fs::write(&summary, text).map_err(|source| spawn_error("write step summary", source))?;
    }
    if committed.toolchain != measured.toolchain
        || rows.iter().any(|row| row.status != Status::Within)
    {
        return Err(error(
            "instruction budgets are out of date; see the table above",
        ));
    }
    Ok(())
}

/// Reads `--update` and `--out <dir>`; anything else is a usage error.
fn parse(args: &[String]) -> Result<Options, DevtoolError> {
    let mut options = Options {
        update: false,
        out: None,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--update" => options.update = true,
            "--out" => {
                options.out = Some(
                    args.next()
                        .ok_or_else(|| error("--out requires a directory"))?
                        .into(),
                );
            }
            unknown => return Err(error(format!("unknown option {unknown}"))),
        }
    }
    Ok(options)
}

/// Gives this command's failures their own searchable prefix.
fn error(message: impl std::fmt::Display) -> DevtoolError {
    DevtoolError::Usage(format!("instruction-budgets: {message}"))
}

/// Wraps an I/O failure with the step that hit it.
fn spawn_error(command: &str, source: std::io::Error) -> DevtoolError {
    DevtoolError::Spawn {
        command: command.into(),
        source,
    }
}

/// Runs `command` and returns its stdout, failing with its stderr.
fn output(command: &mut Command, label: &str) -> Result<String, DevtoolError> {
    let result = command
        .output()
        .map_err(|source| spawn_error(label, source))?;
    if !result.status.success() {
        return Err(DevtoolError::Command {
            command: label.into(),
            status: result.status.to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).into(),
        });
    }
    String::from_utf8(result.stdout).map_err(|_| DevtoolError::NonUtf8 {
        command: label.into(),
    })
}

/// The directory of the workspace manifest Cargo resolves from the current directory.
fn workspace_root() -> Result<PathBuf, DevtoolError> {
    let path = output(
        Command::new("cargo").args(["locate-project", "--workspace", "--message-format", "plain"]),
        "cargo locate-project",
    )?;
    Path::new(path.trim())
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| error("workspace manifest has no directory"))
}

/// `rustc -V` of the toolchain the workspace pins.
fn toolchain(root: &Path) -> Result<String, DevtoolError> {
    Ok(output(
        Command::new("rustc").arg("-V").current_dir(root),
        "rustc -V",
    )?
    .trim()
    .to_owned())
}

/// Lists budget tests through nextest, which builds their binaries with the caller's profile.
fn discover(root: &Path) -> Result<Vec<BudgetTest>, DevtoolError> {
    let filter = format!("test(/(^|::){}/)", test_instructions::TEST_PREFIX);
    let listing = output(
        Command::new("cargo")
            .args([
                "nextest",
                "list",
                "--workspace",
                "--locked",
                "--message-format",
                "json",
            ])
            .args(["-E", &filter])
            .current_dir(root),
        "cargo nextest list",
    )?;
    parse_listing(&listing)
}

/// Selects the tests nextest's JSON listing marks as matching its filter.
fn parse_listing(listing: &str) -> Result<Vec<BudgetTest>, DevtoolError> {
    let listing: serde_json::Value = serde_json::from_str(listing)?;
    let suites = listing["rust-suites"]
        .as_object()
        .ok_or_else(|| error("nextest listing has no rust-suites"))?;
    let mut tests = Vec::new();
    for (binary_id, suite) in suites {
        let Some(cases) = suite["testcases"].as_object() else {
            continue;
        };
        for (name, case) in cases {
            if case["filter-match"]["status"] != "matches" {
                continue;
            }
            let field = |key: &str| {
                suite[key]
                    .as_str()
                    .map(PathBuf::from)
                    .ok_or_else(|| error(format!("suite {binary_id} has no {key}")))
            };
            tests.push(BudgetTest {
                id: format!("{binary_id} {name}"),
                name: name.clone(),
                binary: field("binary-path")?,
                cwd: field("cwd")?,
            });
        }
    }
    tests.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tests)
}

/// Runs one test under Callgrind and returns the instructions its counted region retired.
fn measure(test: &BudgetTest, profile: &Path) -> Result<u64, DevtoolError> {
    let result = Command::new("valgrind")
        .arg("--tool=callgrind")
        .arg("--collect-atstart=no")
        .arg(format!(
            "--toggle-collect={}",
            test_instructions::REGION_SYMBOL
        ))
        .arg(format!("--callgrind-out-file={}", profile.display()))
        .arg(&test.binary)
        .args(["--exact", &test.name, "--test-threads=1"])
        .current_dir(&test.cwd)
        .env("CARGO_MANIFEST_DIR", &test.cwd)
        .env("GLIBC_TUNABLES", GLIBC_TUNABLES)
        .output()
        .map_err(|source| spawn_error("valgrind (install Valgrind to measure budgets)", source))?;
    if !result.status.success() {
        return Err(DevtoolError::Command {
            command: format!("valgrind {}", test.id),
            status: result.status.to_string(),
            stderr: format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ),
        });
    }
    let text = fs::read_to_string(profile)
        .map_err(|source| spawn_error("read Callgrind output", source))?;
    callgrind_total(&text)
        .filter(|total| *total > 0)
        .ok_or_else(|| {
            error(format!(
                "{} never entered test_instructions::count; did it run?",
                test.id
            ))
        })
}

/// Reads the collected instruction total from a Callgrind profile.
fn callgrind_total(profile: &str) -> Option<u64> {
    profile.lines().find_map(|line| {
        line.strip_prefix("totals:")
            .or_else(|| line.strip_prefix("summary:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

/// Turns a test id into a portable file name.
fn file_stem(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '.'
            }
        })
        .collect()
}

/// Writes budgets as stable, pretty-printed JSON ending in a newline.
fn write_budgets(path: &Path, budgets: &Budgets) -> Result<(), DevtoolError> {
    let mut text = serde_json::to_string_pretty(budgets)?;
    text.push('\n');
    fs::write(path, text)
        .map_err(|source| spawn_error(&format!("write {}", path.display()), source))
}

/// Pairs committed budgets with measurements; a change past the tolerance either way fails.
fn compare(committed: &Budgets, measured: &Budgets) -> Vec<Row> {
    let mut ids: Vec<&String> = committed
        .budgets
        .keys()
        .chain(measured.budgets.keys())
        .collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let budget = committed.budgets.get(id).copied();
            let value = measured.budgets.get(id).copied();
            let status = match (budget, value) {
                (Some(budget), Some(value)) => {
                    let change = percent_change(budget, value);
                    if change > TOLERANCE_PERCENT {
                        Status::Over
                    } else if change < -TOLERANCE_PERCENT {
                        Status::Under
                    } else {
                        Status::Within
                    }
                }
                (None, _) => Status::Unbudgeted,
                (_, None) => Status::Stale,
            };
            Row {
                id: id.clone(),
                budget,
                measured: value,
                status,
            }
        })
        .collect()
}

/// Signed change from `budget` to `measured` in percent; any count over a zero budget is infinite.
fn percent_change(budget: u64, measured: u64) -> f64 {
    if budget == 0 {
        return if measured == 0 { 0.0 } else { f64::INFINITY };
    }
    (measured as f64 - budget as f64) / budget as f64 * 100.0
}

/// Renders a Markdown table, plus update instructions when anything is out of date.
fn report(committed: &Budgets, measured: &Budgets, rows: &[Row]) -> String {
    let cell = |value: Option<u64>| value.map_or_else(|| "-".to_owned(), |value| value.to_string());
    let mut text = format!(
        "## Instruction budgets\n\nTolerance: ±{TOLERANCE_PERCENT}%.\n\n\
         | Test | Budget | Measured | Change | Status |\n| --- | ---: | ---: | ---: | --- |\n"
    );
    for row in rows {
        let change = match (row.budget, row.measured) {
            (Some(budget), Some(value)) => format!("{:+.2}%", percent_change(budget, value)),
            _ => "-".into(),
        };
        let _ = writeln!(
            text,
            "| `{}` | {} | {} | {change} | {} |",
            row.id,
            cell(row.budget),
            cell(row.measured),
            row.status.label()
        );
    }
    if committed.toolchain != measured.toolchain {
        let _ = writeln!(
            text,
            "\nBudgets were measured with `{}`; this run used `{}`.",
            committed.toolchain, measured.toolchain
        );
    }
    if committed.toolchain != measured.toolchain
        || rows.iter().any(|row| row.status != Status::Within)
    {
        let _ = writeln!(
            text,
            "\nIf the change is intended, replace `{BUDGET_FILE}` with this run's \
             `instruction-budgets.json` (CI uploads it as the `instruction-budgets` artifact), or run \
             `{UPDATE_COMMAND}` on x86-64 Linux with Valgrind. Explain increases in the pull request. \
             `callgrind_annotate` on the uploaded `.callgrind` files attributes the change."
        );
    }
    text
}

#[cfg(test)]
mod tests;
