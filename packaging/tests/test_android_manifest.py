"""Check Android manifest declarations against the pinned SDK platform."""

import json
import os
from pathlib import Path
import re
import subprocess
import unittest
import xml.etree.ElementTree as ElementTree


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "packaging/android/AndroidManifest.xml"
ANDROID = "{http://schemas.android.com/apk/res/android}"


def activity_config_changes():
    """Maps each manifest activity name to the set of configChanges values it handles itself."""
    activities = ElementTree.parse(MANIFEST).getroot().iter("activity")
    return {activity.get(ANDROID + "name"): set(activity.get(ANDROID + "configChanges", "").split("|"))
            for activity in activities}


def platform_config_changes(aapt2, android_jar):
    """Returns every public configChanges flag that the given platform's android.jar declares."""
    dump = subprocess.check_output([str(aapt2), "dump", "resources", str(android_jar)], text=True)
    section = re.search(r"(?m)^ *resource 0x[0-9a-f]+ attr/configChanges PUBLIC\n((?: {6,}.*\n)+)", dump)
    if section is None:
        raise AssertionError("android.jar declares no public configChanges attribute")
    return set(re.findall(r"(?m)^ {8,}(\w+)\(0x[0-9a-f]+\)=0x[0-9a-f]+$", section[1]))


class AndroidManifestTests(unittest.TestCase):
    def test_activities_share_one_config_change_set(self):
        """Both activities keep running through the same configuration changes."""
        changes = activity_config_changes()
        self.assertEqual(set(changes), {".BootstrapActivity", ".CinnabarActivity"})
        self.assertEqual(changes[".BootstrapActivity"], changes[".CinnabarActivity"])

    def test_activities_handle_every_platform_config_change(self):
        """A configuration change must never recreate the game activity, whose destruction ends the process."""
        sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
        if not sdk:
            self.skipTest("missing fixture: Android SDK (ANDROID_HOME)")
        pins = json.loads((ROOT / "packaging/android/runtime.json").read_text())
        aapt2 = Path(sdk) / "build-tools" / pins["build_tools"] / "aapt2"
        android_jar = Path(sdk) / "platforms" / f'android-{pins["target_sdk"]}' / "android.jar"
        if not aapt2.is_file() or not android_jar.is_file():
            self.skipTest("missing fixture: pinned Android SDK platform and build-tools")
        platform = platform_config_changes(aapt2, android_jar)
        self.assertIn("orientation", platform)
        for name, changes in activity_config_changes().items():
            with self.subTest(activity=name):
                self.assertEqual(platform - changes, set())
                self.assertEqual(changes - platform, set())


if __name__ == "__main__":
    unittest.main()
