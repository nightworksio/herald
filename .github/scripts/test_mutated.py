"""mutated.py's scope of a ci run: python3 -m unittest discover .github/scripts"""
import re
import unittest

import mutated

ROOT = "/w"


def package(name: str, *on: str) -> dict:
    return {
        "name": name,
        "manifest_path": f"{ROOT}/crates/{name}/Cargo.toml",
        "dependencies": [{"name": dependency, "path": f"{ROOT}/crates/{dependency}"} for dependency in on]
        + [{"name": "serde"}],
    }


METADATA = {
    "workspace_root": ROOT,
    "packages": [
        package("herald-core"),
        package("herald-ports", "herald-core"),
        package("herald-flows", "herald-ports"),
        package("herald-testing", "herald-core"),
        package("herald-wasm", "herald-flows"),
        package("herald-store-sqlite"),
    ],
}


def changing(*paths: str):
    return lambda green: list(paths) if green == "g1" else []


class Members(unittest.TestCase):
    def test_each_member_is_named_by_its_directory(self):
        self.assertEqual(mutated.members(METADATA)["crates/herald-ports/"], "herald-ports")
        self.assertEqual(len(mutated.members(METADATA)), 6)


class Changed(unittest.TestCase):
    CRATES = mutated.members(METADATA)

    def test_a_path_in_a_crate_names_it_and_an_unread_one_names_nothing(self):
        found = mutated.changed(
            ["crates/herald-core/src/lib.rs", ".docs/decisions/0001-x.md", ".github/workflows/hygiene.yml",
             "crates/herald-store-sqlite/tests/a.rs", "LICENSE-MIT"],
            self.CRATES,
        )
        self.assertEqual(found, {"herald-core", "herald-store-sqlite"})

    def test_a_path_outside_every_crate_a_build_or_test_may_read_is_named(self):
        for path in ["Cargo.lock", "Cargo.toml", ".docs/settings.md", "README.md", ".cargo/mutants.toml",
                     ".github/workflows/ci.yml", ".github/scripts/install-tool.sh", ".github/dogfood/herald.toml",
                     "deploy/tests/fixture.mjs",
                     "crates/herald-gone/src/lib.rs"]:
            self.assertEqual(mutated.changed(["crates/herald-core/src/lib.rs", path], self.CRATES), path)


class Dependents(unittest.TestCase):
    def test_every_crate_depending_on_a_changed_one_is_reached_through_another(self):
        self.assertEqual(
            mutated.dependents(METADATA, frozenset({"herald-ports"})),
            {"herald-ports", "herald-flows", "herald-wasm"},
        )
        self.assertEqual(mutated.dependents(METADATA, frozenset()), frozenset())


class LastGreen(unittest.TestCase):
    def test_the_newest_run_whose_commit_is_held_is_taken(self):
        runs = {"workflow_runs": [{"head_sha": "gone"}, {"head_sha": "g1"}, {"head_sha": "g0"}]}
        self.assertEqual(mutated.last_green(runs, lambda sha: sha != "gone"), "g1")
        self.assertIsNone(mutated.last_green({"workflow_runs": [{"head_sha": "gone"}]}, lambda sha: False))
        self.assertIsNone(mutated.last_green({}, lambda sha: True))


class Nightly(unittest.TestCase):
    def test_no_green_run_mutates_the_whole_workspace(self):
        found = mutated.nightly(None, changing(), METADATA)
        self.assertTrue(found.whole)
        self.assertEqual(found.outputs(), {"shards": "--workspace", "wasm": "true", "wasi": "true", "fastly": "true"})

    def test_a_read_path_outside_the_crates_mutates_the_whole_workspace(self):
        found = mutated.nightly("g1", changing("crates/herald-core/src/lib.rs", "Cargo.lock"), METADATA)
        self.assertTrue(found.whole)
        self.assertIn("Cargo.lock changed since g1", found.why)

    def test_the_changed_crates_and_their_dependents_are_mutated_the_runtimes_by_their_own_jobs(self):
        found = mutated.nightly("g1", changing("crates/herald-ports/src/lib.rs"), METADATA)
        self.assertFalse(found.whole)
        self.assertEqual(
            found.outputs(),
            {"shards": "--package herald-flows --package herald-ports --package herald-wasm", "wasm": "true",
             "wasi": "false", "fastly": "false"},
        )
        self.assertIn("herald-flows herald-ports herald-wasm", found.why)

    def test_a_crate_the_shards_exclude_whole_is_left_to_its_runtime_job(self):
        metadata = {"workspace_root": ROOT, "packages": [package("herald-wasi"), package("herald-spin")]}
        found = mutated.nightly("g1", changing("crates/herald-wasi/src/lib.rs", "crates/herald-spin/src/lib.rs"), metadata)
        self.assertEqual(found.outputs(), {"shards": "", "wasm": "false", "wasi": "true", "fastly": "false"})

    def test_nothing_changed_mutates_nothing(self):
        found = mutated.nightly("g1", changing(".docs/decisions/0002-y.md"), METADATA)
        self.assertEqual(found.outputs(), {"shards": "", "wasm": "false", "wasi": "false", "fastly": "false"})
        self.assertEqual(found.why, "no crate changed since g1, so nothing is mutated")


class Scope(unittest.TestCase):
    def test_every_run_but_the_nightly_mutates_the_whole_workspace(self):
        for event, schedule, ref in [("schedule", mutated.WEEKLY, mutated.MAIN), ("pull_request", "", "refs/pull/1/merge"),
                                     ("workflow_dispatch", "", mutated.MAIN), ("push", "", "refs/tags/v1.0.0"),
                                     ("", "", ""), ("schedule", "0 0 * * *", mutated.MAIN)]:
            self.assertTrue(mutated.scope(event, schedule, ref).whole, event)
        self.assertEqual(mutated.scope("schedule", mutated.WEEKLY, mutated.MAIN).why,
                         "the weekly run mutates the whole workspace")

    def test_a_push_to_main_mutates_nothing(self):
        found = mutated.scope("push", "", mutated.MAIN)
        self.assertEqual(found.outputs(), {"shards": "", "wasm": "false", "wasi": "false", "fastly": "false"})


FORGE_PLACES = frozenset({".github/CODEOWNERS"})


class Repository(unittest.TestCase):
    def test_the_workflow_is_scheduled_on_the_nightly_and_the_weekly_cron_alone(self):
        workflow = (mutated.ROOT / ".github" / "workflows" / mutated.WORKFLOW).read_text(encoding="utf-8")
        self.assertEqual(re.findall(r'- cron: "([^"]+)"', workflow), [mutated.NIGHTLY, mutated.WEEKLY])

    def test_the_shards_exclude_every_crate_held_unsharded(self):
        workflow = (mutated.ROOT / ".github" / "workflows" / mutated.WORKFLOW).read_text(encoding="utf-8")
        for name in mutated.UNSHARDED:
            self.assertIn(f"--exclude 'crates/{name}/**'", workflow)

    def test_no_crate_names_a_path_held_unread(self):
        literal = re.compile(r'"(?:\.\./)*([^"\s]+)"')
        found = [
            f"{path}: {match.group(1)}"
            for path in (mutated.ROOT / "crates").rglob("*.rs")
            for match in literal.finditer(path.read_text(encoding="utf-8"))
            if mutated.unread(match.group(1)) and match.group(1) not in FORGE_PLACES
        ]
        self.assertEqual(found, [])

if __name__ == "__main__":
    unittest.main()
