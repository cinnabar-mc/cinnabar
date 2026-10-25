"""Exercise Android Cargo build commands without an SDK or native build."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AndroidFeatureTests(unittest.TestCase):
    def test_client_and_compiler_build_without_optional_features(self):
        """Build both hosts on the available Bash versions without optional feature arguments."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["Cargo.toml", "packaging/android/build-apk.sh",
                         "packaging/android/runtime.json", "packaging/android/client/Cargo.toml",
                         "packaging/common/stage-payload.sh", "crates/launcher/src/lib.rs"]:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, target)
            binary = root / "bin"
            binary.mkdir()
            cargo = binary / "cargo"
            cargo.write_text("#!/usr/bin/env python3\n"
                             "import json, os, sys\n"
                             "with open(os.environ['FEATURE_LOG'], 'a') as output:\n"
                             "    output.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                             "if sys.argv[1] == 'metadata': print(json.dumps({'target_directory': 'unused'}))\n"
                             "raise SystemExit(0 if sys.argv[1] == 'ndk' else 77)\n")
            cargo.chmod(0o755)
            log = root / "commands.jsonl"
            shells = {shutil.which("bash")}
            if Path("/bin/bash").is_file():
                shells.add("/bin/bash")
            for shell in sorted(shell for shell in shells if shell):
                log.write_text("")
                env = {**os.environ, "PATH": str(binary) + os.pathsep + os.environ["PATH"],
                       "ANDROID_HOME": str(root / "android sdk"), "FEATURE_LOG": str(log),
                       "CINNABAR_BUILD_COMMIT": "fixture"}
                env.pop("CINNABAR_SOURCE_ROOT", None)
                result = subprocess.run([shell, "packaging/android/build-apk.sh"], cwd=root,
                                        env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 77, result.stderr)
                builds = [json.loads(line) for line in log.read_text().splitlines()]
                builds = [args for args in builds if args[0] == "ndk"]
                self.assertEqual(len(builds), 2)
                for args in builds:
                    self.assertNotIn("--features", args)
                self.assertEqual(builds[1][builds[1].index("-p") + 1], "asset-compiler")


if __name__ == "__main__":
    unittest.main()
