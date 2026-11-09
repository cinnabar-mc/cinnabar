"""Exercise the Cinnabar API release notice offline with a fake curl."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("manifest_publish", Path(__file__).with_name("test_manifest_publish.py"))
manifest_publish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(manifest_publish)
STEP = "Announce the release to the Cinnabar API"
REPORT_STEP = "Report supported mod API revisions to the Cinnabar API"
SECRET = "hook-secret-value"
FAKE_CURL = '''#!/bin/bash
printf '%s\\n' "$@" > argv
previous=
for argument in "$@"; do
  if [[ "$previous" == -H && "$argument" == @* ]]; then cat "${argument#@}" >> headers; fi
  [[ "$previous" != --data ]] || printf '%s' "$argument" > body
  previous="$argument"
done
printf '%s' "${FAKE_STATUS:-204}"
'''


class ReleaseHookTests(unittest.TestCase):
    def setUp(self):
        """Builds a workspace whose release tooling holds the shared contract manifest."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        contract = self.root / "release-tools/crates/bridge/data/contract.json"
        contract.parent.mkdir(parents=True)
        contract.write_text((ROOT / "crates/bridge/data/contract.json").read_text())
        (self.root / "curl").write_text(FAKE_CURL)
        (self.root / "curl").chmod(0o755)
        self.revisions = json.loads((ROOT / "crates/mod-api/compatibility-revisions.json").read_text())
        revisions = self.root / "crates/mod-api/compatibility-revisions.json"
        revisions.parent.mkdir(parents=True)
        revisions.write_text(json.dumps(self.revisions))

    def announce(self, secret=SECRET, status="204", published="true"):
        """Runs the workflow step against the fake curl."""
        env = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}", GITHUB_WORKSPACE=str(self.root),
                   RELEASE_CHANNEL="nightly", RELEASE_VERSION="0.1.18", GITHUB_RUN_NUMBER="42", MANIFEST_PUBLISHED=published, FAKE_STATUS=status,
                   CINNABAR_API_RELEASE_HOOK_SECRET=secret)
        result = subprocess.run(["bash", "-e", "-c", manifest_publish.step(STEP)], cwd=self.root, env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def test_posts_the_published_channel_without_exposing_the_secret(self):
        result = self.announce()
        base = next(row["value"] for row in json.loads((ROOT / "crates/bridge/data/contract.json").read_text())["constants"]
                    if row["go_name"] == "CinnabarAPIBaseURL")
        argv = (self.root / "argv").read_text()
        self.assertIn(f"{base}/v1/releases", argv.splitlines())
        self.assertNotIn(SECRET, argv)
        self.assertNotIn(SECRET, result.stdout + result.stderr)
        self.assertEqual((self.root / "headers").read_text().strip(), f"Authorization: Bearer {SECRET}")
        self.assertEqual(json.loads((self.root / "body").read_text()), {"channel": "nightly", "version": "0.1.18", "build": 42})

    def test_a_failed_notice_warns_without_failing_the_release(self):
        result = self.announce(status="500")
        self.assertIn("::warning::Cinnabar API release notice failed (HTTP 500)", result.stdout)

    def test_an_unpublished_manifest_does_not_announce(self):
        """The step guards its own execution as well as the workflow's output condition."""
        self.announce(published="")
        self.assertFalse((self.root / "argv").exists())

    def test_a_missing_secret_skips_the_notice(self):
        result = self.announce(secret="")
        self.assertFalse((self.root / "argv").exists())
        self.assertIn("::warning::", result.stdout)

    def report(self, secret=SECRET, status="204"):
        """Runs the revision report against the fake curl and returns the finished process."""
        env = dict(os.environ, PATH=f"{self.root}:{os.environ['PATH']}", GITHUB_WORKSPACE=str(self.root),
                   RELEASE_CHANNEL="nightly", RELEASE_VERSION="0.1.18", FAKE_STATUS=status,
                   CINNABAR_API_RELEASE_HOOK_SECRET=secret)
        return subprocess.run(["bash", "-e", "-c", manifest_publish.step(REPORT_STEP)], cwd=self.root, env=env,
                              capture_output=True, text=True)

    def test_reports_the_published_builds_revisions_without_exposing_the_secret(self):
        result = self.report()
        self.assertEqual(result.returncode, 0, result.stderr)
        argv = (self.root / "argv").read_text()
        self.assertTrue(any(line.endswith("/v1/mod/host-revisions") for line in argv.splitlines()))
        self.assertNotIn(SECRET, argv)
        self.assertNotIn(SECRET, result.stdout + result.stderr)
        self.assertEqual((self.root / "headers").read_text().strip(), f"Authorization: Bearer {SECRET}")
        self.assertEqual(json.loads((self.root / "body").read_text()),
                         {"channel": "nightly", "mod_api_revisions": self.revisions})

    def test_a_source_without_the_revision_file_reports_none(self):
        (self.root / "crates/mod-api/compatibility-revisions.json").unlink()
        result = self.report()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.root / "body").read_text()), {"channel": "nightly", "mod_api_revisions": []})

    def test_a_malformed_revision_file_fails_without_reporting(self):
        (self.root / "crates/mod-api/compatibility-revisions.json").write_text("not json")
        self.assertNotEqual(self.report().returncode, 0)
        self.assertFalse((self.root / "argv").exists())

    def test_a_failed_report_fails_so_that_the_job_can_be_rerun(self):
        result = self.report(status="500")
        self.assertEqual(result.returncode, 1)
        self.assertIn("::error::Mod API revision report failed (HTTP 500)", result.stdout)

    def test_a_missing_secret_skips_the_report(self):
        result = self.report(secret="")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.root / "argv").exists())
        self.assertIn("::warning::", result.stdout)


if __name__ == "__main__":
    unittest.main()
