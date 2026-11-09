"""Fail unless every version-matched Bevy crate pins one commit on its fork release branch."""

from pathlib import Path
import re
import sys
import tomllib

from wgpu_fork_pin import compare_status, on_branch

FORK_URL = "https://github.com/cinnabar-mc/bevy"


def pin(manifest: dict) -> tuple[str, str, str]:
    """Return the repository, exact revision and release branch for the workspace's Bevy version."""
    version = manifest["workspace"]["dependencies"]["bevy"]["version"].removeprefix("=")
    release = ".".join(version.split(".")[:2])
    patches = {
        name: entry for name, entry in manifest["patch"]["crates-io"].items()
        if name == "bevy" or name.startswith("bevy_")
    }
    pins = {(entry.get("git"), entry.get("rev")) for entry in patches.values()}
    if len(pins) != 1 or not patches:
        raise ValueError("Bevy fork patches must share one git URL and rev")
    (url, rev), = pins
    if url != FORK_URL or not isinstance(rev, str) or not re.fullmatch(r"[0-9a-f]{40}", rev):
        raise ValueError("Bevy patches must use the Cinnabar fork and a full commit ID")
    return url.removeprefix("https://github.com/"), rev, f"cinnabar-v{release}"


def check_lock(manifest: dict, lock: dict, rev: str) -> None:
    """Reject a newly added Bevy crate unless it shares the checked fork pin and patch entry."""
    version = manifest["workspace"]["dependencies"]["bevy"]["version"].removeprefix("=")
    patches = manifest["patch"]["crates-io"]
    for package in lock["package"]:
        name = package["name"]
        if package["version"] != version or not (name == "bevy" or name.startswith("bevy_")):
            continue
        if name not in patches or package.get("source") != f"git+{FORK_URL}?rev={rev}#{rev}":
            raise ValueError(f"{name} must share the Bevy fork patch and locked source")


def main() -> int:
    """Check the manifest, locked Bevy graph and reachability from the fork's release branch."""
    path = Path(sys.argv[1] if len(sys.argv) > 1 else "Cargo.toml")
    manifest = tomllib.loads(path.read_text())
    repository, rev, branch = pin(manifest)
    check_lock(manifest, tomllib.loads(path.with_name("Cargo.lock").read_text()), rev)
    status = compare_status(repository, branch, rev)
    if not on_branch(status):
        print(f"{rev} is not on {repository}:{branch} ({status}); merge it there before pinning it.", file=sys.stderr)
        return 1
    print(f"Bevy fork pin {rev[:9]} is on {repository}:{branch}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
