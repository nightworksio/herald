"""The deciding half of release.py: python3 -m unittest discover .github/scripts"""
import unittest

import release

CHANGELOG = """# Changelog

Every release.

## [0.2.0] - 2026-11-02

### Added

- **store:** Something (#70)

## [0.2.0-rc.1] - 2026-11-01

### Added

- A candidate (#69)

## [0.1.0] - 2026-10-31

### Added

- The first (#1)
"""


class Level(unittest.TestCase):
    def test_a_feature_is_a_minor_and_a_fix_or_perf_a_patch(self):
        self.assertEqual(release.level("feat(store): keep more (#1)"), "minor")
        self.assertEqual(release.level("fix: a thing (#2)"), "patch")
        self.assertEqual(release.level("perf(core): faster (#3)"), "patch")

    def test_a_bang_or_a_breaking_footer_is_a_major(self):
        self.assertEqual(release.level("refactor(config)!: rename (#4)"), "major")
        self.assertEqual(release.level("fix: moved (#5)\n\nBREAKING CHANGE: the key moved"), "major")

    def test_every_other_commit_asks_nothing(self):
        for message in ("docs: words (#6)", "chore: tidy", "refactor: inside", "Merge branch 'x'"):
            self.assertIsNone(release.level(message))


class NextVersion(unittest.TestCase):
    def test_the_highest_level_since_the_last_tag_moves_the_version(self):
        self.assertEqual(release.next_version("v1.2.3", ["fix: a", "docs: b"], ""), "1.2.4")
        self.assertEqual(release.next_version("v1.2.3", ["fix: a", "feat: b"], ""), "1.3.0")
        self.assertEqual(release.next_version("v1.2.3", ["feat!: a", "fix: b"], ""), "2.0.0")

    def test_below_one_a_breaking_change_is_a_minor(self):
        self.assertEqual(release.next_version("v0.4.1", ["feat!: a"], ""), "0.5.0")

    def test_after_a_pre_release_comes_its_release(self):
        self.assertEqual(release.next_version("v0.2.0-rc.1", ["fix: a"], ""), "0.2.0")

    def test_a_given_version_wins_where_it_comes_after_the_last(self):
        self.assertEqual(release.next_version("v0.9.0", ["fix: a"], "1.0.0"), "1.0.0")
        self.assertEqual(release.next_version(None, [], "v0.1.0"), "0.1.0")
        self.assertEqual(release.next_version("v0.1.0", [], "0.2.0-rc.1"), "0.2.0-rc.1")
        self.assertEqual(release.next_version("v0.2.0-rc.1", [], "0.2.0-rc.2"), "0.2.0-rc.2")
        self.assertEqual(release.next_version("v0.2.0-rc.2", [], "0.2.0"), "0.2.0")

    def test_refuses_what_cannot_be_released(self):
        cases = [
            ("v1.0.0", ["docs: a"], ""),
            (None, ["feat: a"], ""),
            ("v1.2.0", [], "1.1.9"),
            ("v1.2.0", [], "2.0"),
            ("v1.2.0", [], "1.2.0-rc.1"),
            ("v1.2.0", [], "1.2.0"),
        ]
        for last, messages, given in cases:
            with self.assertRaises(release.Refused):
                release.next_version(last, messages, given)


class Changelog(unittest.TestCase):
    def test_puts_a_section_above_every_earlier_one_below_the_header(self):
        written = release.prepend(CHANGELOG, "## [0.3.0] - 2026-11-03\n\n### Fixed\n\n- A fix (#71)\n")
        self.assertTrue(written.startswith("# Changelog\n\nEvery release.\n\n## [0.3.0] - 2026-11-03\n"))
        self.assertIn("- A fix (#71)\n\n## [0.2.0] - 2026-11-02\n", written)

    def test_puts_the_first_section_below_the_header(self):
        written = release.prepend("# Changelog\n\nEvery release.\n", "## [0.1.0] - 2026-10-31\n\n- One\n")
        self.assertEqual(written, "# Changelog\n\nEvery release.\n\n## [0.1.0] - 2026-10-31\n\n- One\n")

    def test_the_notes_are_the_tags_section_without_its_heading(self):
        self.assertEqual(release.notes(CHANGELOG, "v0.2.0"), "### Added\n\n- **store:** Something (#70)\n")
        self.assertEqual(release.notes(CHANGELOG, "v0.2.0-rc.1"), "### Added\n\n- A candidate (#69)\n")
        self.assertEqual(release.notes(CHANGELOG, "v0.1.0"), "### Added\n\n- The first (#1)\n")

    def test_refuses_a_tag_without_a_section(self):
        with self.assertRaises(release.Refused):
            release.notes(CHANGELOG, "v0.1.1")
        with self.assertRaises(release.Refused):
            release.notes("## [2.0.0] - 2026-10-03\n\n## [1.0.0]\n", "v2.0.0")


class Line(unittest.TestCase):
    def test_a_release_line_is_the_major_and_minor_while_the_major_is_0(self):
        self.assertEqual(release.line("v0.1.0"), "0.1")
        self.assertEqual(release.line("0.1.4"), "0.1")
        self.assertEqual(release.line("v1.4.0"), "1")
        with self.assertRaises(release.Refused):
            release.line("v0.1")

    def test_a_pre_release_moves_no_line(self):
        self.assertEqual(release.line("v0.2.0-rc.1"), "")


class Outputs(unittest.TestCase):
    def test_each_value_is_one_line(self):
        self.assertEqual(release.outputs({"line": "0.1", "prerelease": "false"}), "line=0.1\nprerelease=false\n")
        with self.assertRaises(release.Refused):
            release.outputs({"version": "1\nline=evil"})


if __name__ == "__main__":
    unittest.main()
