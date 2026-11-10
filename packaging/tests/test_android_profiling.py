"""Exercise the diagnostic APK configuration without building the client."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
ANDROID = "{http://schemas.android.com/apk/res/android}"


class AndroidProfilingTests(unittest.TestCase):
    def render(self, mode=None, profile="release"):
        """Configure packaging and parse its rendered manifest and effective Cargo settings."""
        environment = dict(os.environ, FIXTURE_PROFILE=profile,
                           CARGO_PROFILE_RELEASE_DEBUG="false",
                           CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO="packed",
                           CARGO_PROFILE_RELEASE_STRIP="debuginfo",
                           RUSTFLAGS="-C opt-level=3")
        environment.pop("CINNABAR_ANDROID_PROFILEABLE", None)
        if mode is not None:
            environment["CINNABAR_ANDROID_PROFILEABLE"] = mode
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([
                "bash", "-euc", '''
profile="$FIXTURE_PROFILE"
source "$1/packaging/android/profiling.sh"
configure_android_profiling
repo_root="$1"
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo_root/packaging/android/runtime.json" "$1"; }
client_library=fixture_client
product=Fixture
version_code=1
version=0.0.1
source "$repo_root/packaging/android/render-template.sh"
render "$repo_root/packaging/android/AndroidManifest.xml" "$2/manifest.xml"
python3 -c 'import json,os; print(json.dumps({key: os.environ[key] for key in ("CARGO_PROFILE_RELEASE_DEBUG", "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO", "CARGO_PROFILE_RELEASE_STRIP", "RUSTFLAGS")}))'
''', "android-profiling-test", str(ROOT), directory,
            ], env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            application = ET.parse(Path(directory) / "manifest.xml").getroot().find("application")
            self.assertNotEqual(application.get(ANDROID + "debuggable"), "true")
            return application.find("profileable").get(ANDROID + "shell"), json.loads(result.stdout)

    def test_default_and_explicit_off_preserve_normal_compiler_settings(self):
        for mode in (None, "false"):
            with self.subTest(mode=mode):
                enabled, settings = self.render(mode)
                self.assertEqual(enabled, "false")
                self.assertEqual(settings, {
                    "CARGO_PROFILE_RELEASE_DEBUG": "false",
                    "CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO": "packed",
                    "CARGO_PROFILE_RELEASE_STRIP": "debuginfo",
                    "RUSTFLAGS": "-C opt-level=3",
                })

    def test_opt_in_enables_profiling_and_retains_matching_native_symbols(self):
        enabled, settings = self.render("true")
        self.assertEqual(enabled, "true")
        self.assertEqual(settings["CARGO_PROFILE_RELEASE_DEBUG"], "line-tables-only")
        self.assertEqual(settings["CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO"], "off")
        self.assertEqual(settings["CARGO_PROFILE_RELEASE_STRIP"], "none")
        self.assertEqual(settings["RUSTFLAGS"].split(),
                         ["-C", "opt-level=3", "-C", "link-arg=-Wl,--build-id=sha1"])

    def test_invalid_modes_fail_before_the_sdk_or_build_is_needed(self):
        for mode, profile, message in (
            ("yes", "release", "must be true or false"),
            ("true", "play", "require CINNABAR_ANDROID_PROFILE=release"),
            ("true", "dev", "require CINNABAR_ANDROID_PROFILE=release"),
        ):
            with self.subTest(mode=mode, profile=profile):
                environment = dict(os.environ, CINNABAR_ANDROID_PROFILEABLE=mode,
                                   CINNABAR_ANDROID_PROFILE=profile)
                environment.pop("ANDROID_HOME", None)
                result = subprocess.run(["bash", str(ROOT / "packaging/android/build-apk.sh")],
                                        env=environment, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
