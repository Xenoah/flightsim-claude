"""Approved text identity and ordinary copy-plan regressions; no actual assent.

The English proposal digest and Japanese section digest identify the reviewed
2026-10-09-2 text. Changing these expectations requires a new substantive review,
not regenerating hashes to accommodate an unreviewed terms edit. Synthetic Git
fixtures are confined to temporary directories by the existing gate test helper.
"""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import unittest


ROOT = Path(__file__).resolve().parents[2]
COMPONENTS = "docs/release/components/"
ENGLISH = COMPONENTS + "MICROSOFT-COMPONENT-TERMS.txt"
JAPANESE = COMPONENTS + "MICROSOFT-COMPONENT-TERMS.ja.txt"
NOTICE = COMPONENTS + "MICROSOFT-COMPONENT-NOTICE.txt"
COMPONENT_FILES = (ENGLISH, JAPANESE, NOTICE)
spec = importlib.util.spec_from_file_location(
    "component_release_fixture", Path(__file__).with_name("test_release_authorization.py")
)
fixture_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture_module)
gate = fixture_module.gate


def digest(value):
    return hashlib.sha256(value).hexdigest()


class ComponentTermsContentTests(unittest.TestCase):
    def test_dialog_suppresses_only_progress_before_module_loading(self):
        source = (ROOT / 'crates/flightsim-app/src/component_terms_dialog.ps1').read_text(encoding='utf-8')
        statements = [line.strip() for line in source.splitlines()
                      if line.strip() and not line.lstrip().startswith('#')]
        self.assertEqual(statements[0], "$ProgressPreference = 'SilentlyContinue'")
        self.assertEqual(statements[1], "$ErrorActionPreference = 'Stop'")
        self.assertEqual(source.count('$ProgressPreference ='), 1)
        self.assertIn("[Console]::Error.WriteLine('Unable to display the complete component terms; no agreement recorded.')", source)
        self.assertIn('    exit 2\n}', source)
        runtime = (ROOT / 'crates/flightsim-app/src/component_terms.rs').read_text(encoding='utf-8')
        self.assertIn('stderr.is_empty()', runtime)

    def test_windows_harness_exercises_progress_and_preserves_real_error_checks(self):
        harness = (ROOT / 'scripts/tests/test-component-terms-dialog.ps1').read_text(encoding='ascii')
        self.assertIn("@('decline', 'decline-with-progress', 'close', 'missing-document')", harness)
        self.assertIn("Write-Progress -Activity 'Synthetic module preparation'", harness)
        self.assertIn('$stderr.Length -ne 0', harness)
        self.assertIn('$process.ExitCode -ne 2 -or $stdout.Length -ne 0 -or $stderr.Length -eq 0', harness)
        self.assertNotIn('$agree.PerformClick()', harness)

    def test_argument_diagnostic_link_names_the_existing_parser(self):
        source = (ROOT / 'crates/flightsim-app/src/main.rs').read_text(encoding='utf-8')
        self.assertIn('fn parse_arguments_from(', source)
        self.assertIn('[`parse_arguments_from`]', source)
        self.assertNotIn('[`parse_arguments`]', source)

    def test_english_matches_reviewed_proposal_with_only_framing_activated(self):
        active = (ROOT / ENGLISH).read_bytes()
        self.assertEqual(len(active), 4537)
        self.assertEqual(digest(active),
                         "052656e63c7f2df77a7cfffb7d78fcbb64d8525f9d0ca69d39e5475e5bce9d20")
        runtime = (ROOT / 'crates/flightsim-app/src/component_terms.rs').read_text(encoding='utf-8')
        declared = re.findall(r'const TERMS_SHA256: &str =\s*"([0-9a-f]{64})";', runtime)
        self.assertEqual(declared, [digest(active)])
        self.assertIn('const TERMS_VERSION: &str = "2026-10-09-2";', runtime)
        # Reverse the three explicitly reviewed framing changes. This compares
        # all original English bytes, not merely selected protective clauses.
        proposal = active.replace(b"Version 2026-10-09-2", b"Candidate version 2026-10-09-2", 1)
        proposal = proposal.replace(
            b"1. Limited scope\n",
            b"INACTIVE PROPOSAL FOR REVIEW. This text has not been published or activated.\n"
            b"The operative text below is the complete proposed recipient supplement.\n\n"
            b"1. Limited scope\n", 1)
        proposal = proposal.replace(b"END OF COMPONENT TERMS", b"END OF PROPOSED OPERATIVE TEXT", 1)
        self.assertEqual(len(proposal), 4705)
        self.assertEqual(digest(proposal),
                         "49876d1ed238424244c7c53307499badd4f8a2c6ef97ac58cb97aacc2490eb08")
        body = active[active.index(b"1. Limited scope\n"):active.index(b"END OF COMPONENT TERMS")]
        self.assertEqual(digest(body),
                         "f035a9c4931f984d7025145826ce2de0047c767d584723b0a7d8bb8c85446c74")

    def test_japanese_preserves_every_approved_section_title_and_paragraph(self):
        active = (ROOT / JAPANESE).read_bytes()
        self.assertEqual(len(active), 6033)
        self.assertEqual(digest(active),
                         "639a4b64da625b37afffd42b9563f0550c3725aeca5940f8a819334cba5e1fde")
        chunks = active.decode("utf-8").rstrip("\n").split("\n\n")
        self.assertEqual(chunks[1], "Version 2026-10-09-2")
        self.assertEqual(chunks[2], "日本語参考訳 / Japanese reference translation")
        self.assertIn("MICROSOFT-COMPONENT-TERMS.txt", chunks[3])
        cursor = 4
        sections = []
        for count in (2, 1, 2, 2, 4, 1):
            sections.append({"title": chunks[cursor], "paragraphs": chunks[cursor + 1:cursor + 1 + count]})
            cursor += count + 1
        self.assertEqual(cursor, len(chunks))
        canonical = json.dumps(sections, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        self.assertEqual(digest(canonical),
                         "eb8e9608799a011820dcc4854376a292db97fd976cf3f5c5c045af55184389a2")

    def test_component_documents_are_utf8_lf_without_inactive_framing(self):
        for relative in COMPONENT_FILES:
            with self.subTest(path=relative):
                raw = (ROOT / relative).read_bytes()
                text = raw.decode("utf-8")
                self.assertNotIn(b"\r", raw)
                self.assertNotIn(b"\xef\xbb\xbf", raw)
                self.assertTrue(raw.endswith(b"\n"))
                for stale in ("INACTIVE", "PROPOSED OPERATIVE", "Candidate version", "draft component"):
                    self.assertNotIn(stale, text)

    def test_notice_requires_actual_build_binding_without_stale_native_claims(self):
        text = (ROOT / NOTICE).read_text(encoding="utf-8")
        for stale in (
            "55094fa928fbb8f907a2103740ee6824171c26e7",
            "50d4254ff3ea5d981b65bed8f1309c332a8c333a817ef2119af09deac58e2626",
            "ee835131ecbe10f52d7a35034183486e68b8e73e2efbfc29e4dd1536eed9da74",
            "14.29.30133", "14.44.35207", "Observed reference", "762 shipped",
        ):
            self.assertNotIn(stale, text)
        for required in (
            "Copyright (c) 2026 flightsim-claude contributors",
            "MIT OR Apache-2.0", "not a measured static object-member inventory",
            "does not assert a selected toolset or exact static membership",
            "separate exact-build release", "source revision/tree, executable SHA-256",
            "release inventory, these component documents", "applicability assessment to be rebound",
            "Unknown static members and toolset selection must remain explicitly unknown",
            "--component-terms", "records no assent",
            "https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170",
        ):
            self.assertIn(required, text)

    def test_pre_download_and_redistribution_disclosures_are_prominent(self):
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertLess(readme.index("Microsoft ランタイムの補足規約と同意方法"), readme.index("## 何ができるか"))
        release = (ROOT / "docs/release/windows-component-release-notes.md").read_text(encoding="utf-8")
        for text in (readme, release):
            for required in ("--component-terms", "Agree and continue", "Decline and exit",
                             "2026-10-09-2", "x64", "MICROSOFT-COMPONENT-TERMS.txt",
                             "MICROSOFT-COMPONENT-TERMS.ja.txt", "MICROSOFT-COMPONENT-NOTICE.txt"):
                self.assertIn(required, text)
        self.assertIn("再配布前", readme)
        self.assertIn("端末内", readme)
        self.assertIn("before onward distribution", release)
        self.assertIn("without starting the simulator", release)
        self.assertIn("local acceptance bound to the exact terms", release)
        self.assertIn("No account, telemetry", release)
        self.assertEqual(release.count("/blob/@SOURCE_SHA@/"), 3)
        self.assertNotIn("/blob/main/", release)


class ComponentTermsCopyPlanTests(unittest.TestCase):
    def setUp(self):
        self.fixture = fixture_module.ReleaseAuthorizationTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()
        for relative in COMPONENT_FILES:
            target = self.fixture.repo / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        self.fixture.commit()

    def test_same_three_exact_documents_enter_existing_ordinary_copy_plan(self):
        _, plan = gate.inspect(self.fixture.repo)
        rows = {row["path"]: row for row in plan["files"]}
        self.assertEqual({path for path in gate.SOURCE_FILES if path.startswith(COMPONENTS)},
                         set(COMPONENT_FILES))
        for relative in COMPONENT_FILES:
            with self.subTest(path=relative):
                self.assertEqual(gate.SOURCE_FILES.count(relative), 1)
                raw = (ROOT / relative).read_bytes()
                self.assertEqual(rows[relative], {"source": relative, "path": relative,
                                                 "bytes": len(raw), "sha256": digest(raw)})
        for original in ("LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md", "README.md"):
            self.assertIn(original, rows)
        self.assertFalse(any(path.startswith("PROPOSED-") for path in rows))

    def test_changed_or_omitted_bundled_terms_and_notice_are_rejected(self):
        _, plan = gate.inspect(self.fixture.repo)
        bundle = self.fixture.root / "bundle"
        bundle.mkdir()
        for row in plan["files"]:
            target = bundle / row["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.fixture.repo / row["source"], target)
        exe = self.fixture.root / "synthetic-never-executed.exe"
        exe.write_bytes(b"Synthetic fixture; not executable")
        shutil.copyfile(exe, bundle / "flightsim-app.exe")
        gate.verify_bundle(bundle, plan, exe)
        for relative in COMPONENT_FILES:
            with self.subTest(path=relative):
                target = bundle / relative
                original = target.read_bytes()
                target.write_bytes(original + b"Unapproved change\n")
                with self.assertRaisesRegex(ValueError, "extra or changed file"):
                    gate.verify_bundle(bundle, plan, exe)
                target.unlink()
                with self.assertRaisesRegex(ValueError, "exactly the authorized payload"):
                    gate.verify_bundle(bundle, plan, exe)
                target.write_bytes(original)

    def test_changed_component_source_invalidates_existing_synthetic_receipt(self):
        for relative in COMPONENT_FILES:
            with self.subTest(path=relative):
                self.fixture.authorize_fixture()
                target = self.fixture.repo / relative
                target.write_bytes(target.read_bytes() + b"Unapproved source change\n")
                self.fixture.commit()
                result, _ = gate.inspect(self.fixture.repo)
                self.assertIn("AUTHORIZATION_STALE_OR_INVALID", {row["code"] for row in result["blockers"]})


if __name__ == "__main__":
    unittest.main()
