"""Fail unless every wgpu and Bevy fork patch pins one commit on its fork's cinnabar-v<version> branch
and every locked crate of that family resolves from it."""

import json
import os
from pathlib import Path
import sys
import tomllib
import urllib.request

FORK_PREFIX = "https://github.com/"
FORKS = ("wgpu", "bevy")


def release_branch(version: str) -> str:
    """The fork branch for a dependency version; pre-1.0 crates break compatibility on the minor."""
    major, minor, *_ = version.lstrip("=^~").split(".")
    return f"cinnabar-v{major}" if major != "0" else f"cinnabar-v0.{minor}"


def pin(manifest: dict, fork: str) -> tuple[str, str, str]:
    """Returns the fork repository, the pinned commit and the release branch for `fork`."""
    version = manifest["workspace"]["dependencies"][fork]["version"]
    patches = [
        entry
        for entry in manifest["patch"]["crates-io"].values()
        if isinstance(entry, dict) and entry.get("git", "").removesuffix(".git").endswith(f"/{fork}")
    ]
    if not patches:
        raise ValueError(f"no {fork} fork patches in [patch.crates-io]")
    pins = {(entry["git"], entry.get("rev")) for entry in patches}
    if len(pins) != 1 or None in {rev for _, rev in pins}:
        raise ValueError(f"{fork} fork patches must share one git URL and rev, found {sorted(map(str, pins))}")
    (url, rev), = pins
    if not url.startswith(FORK_PREFIX):
        raise ValueError(f"unexpected {fork} fork URL {url}")
    return url.removeprefix(FORK_PREFIX).removesuffix(".git"), rev, release_branch(version)


def unpinned(lock: dict, fork: str, repository: str, rev: str) -> list[str]:
    """Locked `fork`, `fork-*` and `fork_*` packages that resolve from somewhere other than the pin.

    Crates that live outside the fork repository, such as `bevy_mikktspace`, are recognised by a
    version that differs from the fork crate's own.
    """
    source = f"git+{FORK_PREFIX}{repository}?rev={rev}#{rev}"
    family = [
        package
        for package in lock["package"]
        if package["name"] == fork or package["name"].startswith((f"{fork}-", f"{fork}_"))
    ]
    versions = {package["version"] for package in family if package["name"] == fork}
    return sorted(
        f"{package['name']} {package['version']}"
        for package in family
        if package["version"] in versions and package.get("source") != source
    )


def on_branch(status: str) -> bool:
    """Whether a GitHub compare of branch...commit means the commit is on the branch."""
    return status in ("identical", "behind")


def compare_status(repository: str, branch: str, rev: str) -> str:
    """Returns GitHub's compare status of the branch against the commit."""
    request = urllib.request.Request(f"https://api.github.com/repos/{repository}/compare/{branch}...{rev}")
    if token := os.environ.get("GH_TOKEN"):
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)["status"]


def main() -> int:
    manifest_path = Path(sys.argv[1] if len(sys.argv) > 1 else "Cargo.toml")
    manifest = tomllib.loads(manifest_path.read_text())
    lock = tomllib.loads((manifest_path.parent / "Cargo.lock").read_text())
    failed = False
    for fork in FORKS:
        repository, rev, branch = pin(manifest, fork)
        if missing := unpinned(lock, fork, repository, rev):
            print(f"{fork} crates outside the fork pin: {', '.join(missing)}; patch them to {rev}.", file=sys.stderr)
            failed = True
        status = compare_status(repository, branch, rev)
        if not on_branch(status):
            print(f"{rev} is not on {repository}:{branch} ({status}); merge it there before pinning it.", file=sys.stderr)
            failed = True
        else:
            print(f"{fork} fork pin {rev[:9]} is on {repository}:{branch}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
