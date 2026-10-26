"""Check exact Bevy pinning, lockfile drift and release-branch reachability."""

import copy
import importlib.util
from pathlib import Path
import sys
import tomllib
import unittest

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("bevy_fork_pin", SCRIPTS / "bevy_fork_pin.py")
fork_pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fork_pin)


def fixture() -> tuple[dict, dict]:
    """Return a small consistent manifest and lockfile with two version-matched Bevy crates."""
    rev = "a" * 40
    patches = {name: {"git": fork_pin.FORK_URL, "rev": rev} for name in ("bevy", "bevy_render")}
    manifest = {"workspace": {"dependencies": {"bevy": {"version": "=0.20.0"}}}, "patch": {"crates-io": patches}}
    lock = {"package": [{"name": name, "version": "0.20.0", "source": f"git+{fork_pin.FORK_URL}?rev={rev}#{rev}"} for name in patches]}
    return manifest, lock


class BevyForkPinTests(unittest.TestCase):
    def test_exact_pin_derives_its_release_branch(self):
        manifest, lock = fixture()
        self.assertEqual(fork_pin.pin(manifest), ("cinnabar-mc/bevy", "a" * 40, "cinnabar-v0.20"))
        fork_pin.check_lock(manifest, lock, "a" * 40)

    def test_split_revisions_and_short_revisions_are_rejected(self):
        for revision in ("b" * 40, "abc123"):
            manifest, _ = fixture()
            manifest["patch"]["crates-io"]["bevy_render"]["rev"] = revision
            with self.assertRaises(ValueError):
                fork_pin.pin(manifest)

    def test_new_unpatched_or_registry_bevy_crate_fails(self):
        manifest, lock = fixture()
        for source in (None, "registry+https://github.com/rust-lang/crates.io-index"):
            modified = copy.deepcopy(lock)
            modified["package"].append({"name": "bevy_new", "version": "0.20.0", "source": source})
            with self.assertRaises(ValueError):
                fork_pin.check_lock(manifest, modified, "a" * 40)

    def test_dependency_pr_head_cannot_pass_the_release_branch_guard(self):
        self.assertTrue(fork_pin.on_branch("identical"))
        self.assertTrue(fork_pin.on_branch("behind"))
        self.assertFalse(fork_pin.on_branch("ahead"))
        self.assertFalse(fork_pin.on_branch("diverged"))

    def test_workspace_graph_uses_the_same_fork_pin(self):
        root = SCRIPTS.parents[1]
        manifest = tomllib.loads((root / "Cargo.toml").read_text())
        _, rev, _ = fork_pin.pin(manifest)
        fork_pin.check_lock(manifest, tomllib.loads((root / "Cargo.lock").read_text()), rev)


if __name__ == "__main__":
    unittest.main()
