"""Run release CPU benchmarks and compare their Criterion median estimates in CI."""

import argparse
import json
import math
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import tomllib


SAMPLE_SIZE = 50
BASELINE_REPEAT_LIMIT = 0.10
VARIANTS = {
    "baseline": {},
    "fat-lto": {"lto": "fat"},
    "x86-64-v2": {"rustflags": "-C target-cpu=x86-64-v2"},
    "mimalloc": {"mimalloc": True},
    "all": {"lto": "fat", "rustflags": "-C target-cpu=x86-64-v2", "mimalloc": True},
    "baseline-repeat": {},
}
# Experiment branch only: measure a subset of variants on any runner OS.
if selected := os.environ.get("RUNTIME_PROFILE_VARIANTS"):
    VARIANTS = {name: VARIANTS[name] for name in selected.split(",")}


def cpu_identity() -> dict:
    """Report the CPU in lscpu's JSON shape, synthesizing it where lscpu is unavailable."""
    if shutil.which("lscpu"):
        return json.loads(subprocess.check_output(["lscpu", "--json"], text=True))
    name = platform.processor() or os.environ.get("PROCESSOR_IDENTIFIER", "unknown")
    return {"lscpu": [{"field": "Model name:", "data": name}]}


BENCHES = {
    "chunk-pipeline": ("chunk_costs", "flight_costs"),
    "meshing": ("chunk_costs", "entity_occlusion"),
    "world": ("chunk_costs", "light_work"),
}


class ComparisonRejected(ValueError):
    """The completed measurements cannot support a valid runtime comparison."""


def configuration(variant: str, root: Path) -> dict:
    """Derive commands and release settings, changing only the variant's requested knobs."""
    settings = VARIANTS[variant]
    profile = {
        "opt-level": 3, "codegen-units": 16, "lto": False, "debug": 0,
        "debug-assertions": False, "overflow-checks": False,
        **tomllib.loads((root / "Cargo.toml").read_text())["profile"]["release"],
    }
    profile["lto"] = settings.get("lto", profile["lto"])
    profile_environment = {
        f"CARGO_PROFILE_RELEASE_{name.upper().replace('-', '_')}":
            str(value).lower() if isinstance(value, bool) else str(value)
        for name, value in profile.items() if not isinstance(value, dict)
    }
    profile_environment["CARGO_INCREMENTAL"] = "0"
    commands = []
    for package, benches in BENCHES.items():
        features = []
        if package != "meshing":
            features.append("runtime-profile")
        if package == "chunk-pipeline":
            features.append("benchmark-support")
        if settings.get("mimalloc"):
            features.append("benchmark-mimalloc")
        for bench in benches:
            command = [
                "cargo", "bench", "--locked", "--profile", "release",
                "--no-default-features", "-p", package, "--bench", bench,
            ]
            if features:
                command += ["--features", ",".join(features)]
            command += ["--", "--sample-size", str(SAMPLE_SIZE), "--noplot"]
            commands.append({"bench": f"{package}/{bench}", "features": features, "command": command})
    return {
        "profile": "release",
        "opt_level": profile["opt-level"],
        "codegen_units": profile["codegen-units"],
        "lto": profile["lto"],
        "rustflags": settings.get("rustflags", ""),
        "allocator": "mimalloc" if settings.get("mimalloc") else "system",
        "profile_environment": profile_environment,
        "commands": commands,
    }


def command_audit(variant: str, config: dict) -> str:
    """Render the exact commands and controlled compiler settings without exposing other env."""
    lines = [
        f"## Runtime configuration: {variant}", "",
        f"Profile: `{config['profile']}`. RUSTFLAGS: `{config['rustflags'] or '(empty)'}`.",
        "", "```text",
        *(f"{name}={value}" for name, value in sorted(config["profile_environment"].items())),
        "```", "",
    ]
    for entry in config["commands"]:
        lines += [
            f"### {entry['bench']}", "",
            "Features: `" + (", ".join(entry["features"]) or "(none)") + "`.",
            "", "```sh", shlex.join(entry["command"]), "```", "",
        ]
    return "\n".join(lines)


def save_metadata(path: Path, metadata: dict) -> None:
    """Record benchmark progress without claiming the unfinished run has completed."""
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(metadata, indent=2) + "\n")
    temporary.replace(path)


def run(variant: str, root: Path) -> None:
    """Attempt every bench sequentially, isolating results and recording command failures."""
    config = configuration(variant, root)
    env = {
        name: value for name, value in os.environ.items()
        if not name.startswith("CARGO_PROFILE_") and name != "CARGO_ENCODED_RUSTFLAGS"
    }
    env.update(config["profile_environment"])
    env["RUSTFLAGS"] = config["rustflags"]
    target = Path(env["CARGO_TARGET_DIR"]).resolve() / f"runtime-profile-{variant}"
    target.mkdir(parents=True, exist_ok=True)
    metadata_path = target / "runtime-profile.json"
    metadata_path.unlink(missing_ok=True)
    env["CARGO_TARGET_DIR"] = str(target / "build")
    env["CARGO_BUILD_BUILD_DIR"] = env["CARGO_TARGET_DIR"]
    if summary_path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary_path).open("a") as stream:
            stream.write(command_audit(variant, config))
    metadata = {
        **config,
        "variant": variant,
        "head_sha": env["GITHUB_SHA"],
        "sample_size": SAMPLE_SIZE,
        "host": platform.node(),
        "rustc": subprocess.check_output(["rustc", "--version", "--verbose"], text=True),
        "cpu": cpu_identity(),
        "completed_benches": [],
        "failed_benches": {},
    }
    save_metadata(metadata_path, metadata)
    for entry in config["commands"]:
        identity = entry["bench"]
        env["CRITERION_HOME"] = str(target / "criterion" / identity)
        try:
            subprocess.run(entry["command"], cwd=root, env=env, check=True)
        except subprocess.CalledProcessError as error:
            metadata["failed_benches"][identity] = f"exit code {error.returncode}"
        except OSError as error:
            metadata["failed_benches"][identity] = f"could not start benchmark: {error}"
        else:
            metadata["completed_benches"].append(identity)
        save_metadata(metadata_path, metadata)
    if metadata["failed_benches"]:
        failures = "; ".join(
            f"{bench}: {reason}" for bench, reason in metadata["failed_benches"].items()
        )
        raise SystemExit(f"Benchmark failures: {failures}")
    metadata["completed"] = True
    save_metadata(metadata_path, metadata)


def suite(root: Path) -> None:
    """Measure every variant on this host with separate builds, then rebuild baseline last."""
    failures = []
    target = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
    for variant in VARIANTS:
        try:
            run(variant, root)
        except (OSError, ValueError, SystemExit) as error:
            failures.append(f"{variant}: {error}")
        finally:
            build = target / f"runtime-profile-{variant}" / "build"
            if build.is_dir():
                shutil.rmtree(build)
    if failures:
        raise SystemExit("Variant failures: " + "; ".join(failures))


def read_results(directory: Path) -> dict[str, float]:
    """Read positive median nanoseconds, rejecting missing benches and wrong sample counts."""
    results = {}
    for package, benches in BENCHES.items():
        for bench in benches:
            paths = sorted(
                (directory / "criterion" / package / bench).glob("**/new/estimates.json")
            )
            if not paths:
                raise ValueError(f"Missing estimates: {directory.name}/{package}/{bench}")
            for path in paths:
                identity = json.loads(path.with_name("benchmark.json").read_text())["full_id"]
                key = f"{package}/{bench}: {identity}"
                median = json.loads(path.read_text())["median"]["point_estimate"]
                sample = json.loads(path.with_name("sample.json").read_text())
                if len(sample["times"]) != SAMPLE_SIZE or len(sample["iters"]) != SAMPLE_SIZE:
                    raise ValueError(f"Wrong sample count: {directory.name}/{key}")
                if (
                    not isinstance(median, (int, float))
                    or not math.isfinite(median)
                    or median <= 0
                ):
                    raise ValueError(f"Invalid median: {directory.name}/{key}")
                if key in results:
                    raise ValueError(f"Duplicate benchmark: {directory.name}/{key}")
                results[key] = median
    return results


def cell(value: str) -> str:
    """Escape text that could otherwise break a Markdown table cell."""
    return (
        value.replace("&", "&amp;").replace("<", "&lt;")
        .replace("|", "&#124;").replace("\n", " ")
    )


def summary(directory: Path) -> str:
    """Compare every case with baseline and retain CPU identity for interpreting runner noise."""
    results = {}
    metadata = {}
    expected_benches = {
        f"{package}/{bench}" for package, benches in BENCHES.items() for bench in benches
    }
    incomplete = []
    for variant in VARIANTS:
        artifact = directory / f"runtime-profile-{variant}"
        path = artifact / "runtime-profile.json"
        data = json.loads(path.read_text()) if path.is_file() else {}
        missing = sorted(expected_benches - set(data.get("completed_benches", [])))
        failures = data.get("failed_benches", {})
        if missing or failures or data.get("completed") is not True:
            detail = ", ".join(missing) if missing else "run completion marker"
            incomplete.append(f"{variant}: missing {detail}")
            incomplete.extend(
                f"{variant}: failed {bench}: {reason}" for bench, reason in failures.items()
            )
        metadata[variant] = data
    if incomplete:
        raise ValueError("Incomplete results; " + "; ".join(incomplete))
    for variant, data in metadata.items():
        artifact = directory / f"runtime-profile-{variant}"
        if (
            data["variant"] != variant
            or data["sample_size"] != SAMPLE_SIZE
            or data["profile"] != "release"
        ):
            raise ValueError(f"Unexpected run metadata: {variant}")
        config = configuration(variant, Path(__file__).resolve().parents[2])
        for name, expected in config.items():
            if data.get(name) != expected:
                raise ComparisonRejected(f"Unexpected {name} configuration: {variant}")
        results[variant] = read_results(artifact)
    baseline = results["baseline"]
    for variant, cases in results.items():
        if metadata[variant]["head_sha"] != metadata["baseline"]["head_sha"]:
            raise ValueError(f"Different source revision: {variant}")
        if metadata[variant]["rustc"] != metadata["baseline"]["rustc"]:
            raise ValueError(f"Different compiler: {variant}")
        if metadata[variant]["host"] != metadata["baseline"]["host"]:
            raise ComparisonRejected(f"Different measurement host: {variant}")
        cpu = {entry["field"]: entry["data"] for entry in metadata[variant]["cpu"]["lscpu"]}
        baseline_cpu = {
            entry["field"]: entry["data"] for entry in metadata["baseline"]["cpu"]["lscpu"]
        }
        if cpu["Model name:"] != baseline_cpu["Model name:"]:
            raise ComparisonRejected(f"Different measurement CPU: {variant}")
        if cases.keys() != baseline.keys():
            missing = sorted(baseline.keys() - cases.keys())
            extra = sorted(cases.keys() - baseline.keys())
            raise ValueError(
                f"Different benchmark cases: {variant}; missing={missing}; extra={extra}"
            )
    repeat_change = math.expm1(math.fsum(
        math.log(results["baseline-repeat"][case]) - math.log(base)
        for case, base in baseline.items()
    ) / len(baseline))
    if abs(repeat_change) > BASELINE_REPEAT_LIMIT + 1e-12:
        raise ComparisonRejected(
            f"Baseline repeat geomean change {repeat_change:+.2%} exceeds "
            f"the {BASELINE_REPEAT_LIMIT:.0%} sanity limit"
        )
    lines = [
        "## Release runtime profile experiment",
        "",
        f"Commit: `{metadata['baseline']['head_sha']}`. {SAMPLE_SIZE} samples per case.",
        "Medians are nanoseconds per iteration. Change = (variant / baseline - 1) × 100%; negative is faster.",
        f"Same-host baseline repeat geomean change: {repeat_change:+.2%} "
        f"(limit: ±{BASELINE_REPEAT_LIMIT:.0%}).",
        "Each variant was rebuilt and measured sequentially on the same host.",
        "",
        "| Benchmark | " + " | ".join(f"{name} median (ns) / change" for name in VARIANTS) + " |",
        "| --- | " + " | ".join("---:" for _ in VARIANTS) + " |",
    ]
    for case, base in sorted(baseline.items()):
        values = [
            f"{results[name][case]:,.2f} / {(results[name][case] / base - 1) * 100:+.2f}%"
            for name in VARIANTS
        ]
        lines.append("| " + cell(case) + " | " + " | ".join(values) + " |")
    lines += [
        "", "| Variant | CPU model | LTO | Rust flags | Allocator |",
        "| --- | --- | --- | --- | --- |",
    ]
    for variant, data in metadata.items():
        fields = {entry["field"]: entry["data"] for entry in data["cpu"]["lscpu"]}
        values = [
            variant, fields.get("Model name:", "unknown"), data["lto"],
            data["rustflags"] or "generic target", data["allocator"],
        ]
        lines.append("| " + " | ".join(cell(value) for value in values) + " |")
    return "\n".join(lines) + "\n"


def main() -> None:
    """Dispatch CI setup, measurements or report generation and expose incomplete results."""
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("suite")
    bench = commands.add_parser("run")
    bench.add_argument("variant", choices=VARIANTS)
    report = commands.add_parser("summary")
    report.add_argument("directory", type=Path)
    report.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.command == "suite":
        suite(Path.cwd())
    elif args.command == "run":
        run(args.variant, Path.cwd())
    else:
        try:
            text = summary(args.directory)
        except ComparisonRejected as error:
            args.output.write_text(f"## Runtime comparison rejected\n\n{cell(str(error))}\n")
            raise SystemExit(str(error)) from error
        except (OSError, KeyError, ValueError) as error:
            args.output.write_text(f"## Runtime measurements incomplete\n\n{cell(str(error))}\n")
            raise SystemExit(str(error)) from error
        args.output.write_text(text)


if __name__ == "__main__":
    main()
