"""Check that the fork pin parser, lock coverage and branch rule reject drift."""

import importlib.util
from pathlib import Path
import tomllib
import unittest


MODULE = Path(__file__).resolve().parents[1] / "scripts/fork_pins.py"
SPEC = importlib.util.spec_from_file_location("fork_pins", MODULE)
fork_pins = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fork_pins)

WGPU = "https://github.com/cinnabar-mc/wgpu"
BEVY = "https://github.com/cinnabar-mc/bevy"


def manifest(wgpu_revs: tuple[str, ...], bevy_rev: str = "c" * 40) -> dict:
    patches = {name: {"git": WGPU, "rev": rev} for name, rev in zip(("wgpu", "naga", "wgpu-hal"), wgpu_revs)}
    patches["bevy"] = {"git": BEVY, "rev": bevy_rev}
    patches["bevy_render"] = {"git": BEVY, "rev": bevy_rev}
    patches["other"] = {"git": "https://github.com/example/other", "rev": "f" * 40}
    return {
        "workspace": {"dependencies": {"wgpu": {"version": "30.0.1"}, "bevy": {"version": "=0.20.0"}}},
        "patch": {"crates-io": patches},
    }


def package(name: str, version: str, source: str) -> dict:
    return {"name": name, "version": version, "source": source}


class ForkPinTests(unittest.TestCase):
    def test_derives_release_branches_from_the_breaking_version(self):
        self.assertEqual(fork_pins.pin(manifest(("a" * 40, "a" * 40)), "wgpu")[2], "cinnabar-v30")
        self.assertEqual(fork_pins.pin(manifest(("a" * 40,)), "bevy"), ("cinnabar-mc/bevy", "c" * 40, "cinnabar-v0.20"))

    def test_rejects_crates_pinned_to_different_commits(self):
        with self.assertRaises(ValueError):
            fork_pins.pin(manifest(("a" * 40, "b" * 40)), "wgpu")

    def test_reports_family_crates_resolved_outside_the_pin(self):
        rev = "c" * 40
        pinned = f"git+{BEVY}?rev={rev}#{rev}"
        registry = "registry+https://github.com/rust-lang/crates.io-index"
        lock = {
            "package": [
                package("bevy", "0.20.0", pinned),
                package("bevy_render", "0.20.0", pinned),
                package("bevy_audio", "0.20.0", registry),
                package("bevy_mikktspace", "1.0.0", registry),
                package("bevyish", "0.20.0", registry),
            ]
        }
        self.assertEqual(fork_pins.unpinned(lock, "bevy", "cinnabar-mc/bevy", rev), ["bevy_audio 0.20.0"])

    def test_only_commits_contained_in_the_branch_pass(self):
        self.assertTrue(fork_pins.on_branch("identical"))
        self.assertTrue(fork_pins.on_branch("behind"))
        self.assertFalse(fork_pins.on_branch("ahead"))
        self.assertFalse(fork_pins.on_branch("diverged"))

    def test_the_workspace_pins_every_fork_crate(self):
        root = Path(__file__).resolve().parents[2]
        workspace = tomllib.loads((root / "Cargo.toml").read_text())
        lock = tomllib.loads((root / "Cargo.lock").read_text())
        for fork in fork_pins.FORKS:
            repository, rev, branch = fork_pins.pin(workspace, fork)
            self.assertEqual(len(rev), 40)
            self.assertTrue(branch.startswith("cinnabar-v"))
            self.assertEqual(fork_pins.unpinned(lock, fork, repository, rev), [])


if __name__ == "__main__":
    unittest.main()
