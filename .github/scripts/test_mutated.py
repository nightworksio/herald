"""mutated.py's scope of a ci run: python3 -m unittest discover .github/scripts"""
import re
import unittest
import unittest.mock

import mutated

ROOT = "/w"
WHOLE = "[" + ",".join(str(index) for index in range(mutated.WHOLE_CUT.most)) + "]"
EXCLUDING = " ".join(
    f"--exclude crates/{glob}" for glob in ("herald-wasm/src/js/**", "herald-fastly/**", "herald-spin/**", "herald-wasi/**")
)


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
        self.assertEqual(found.outputs(), {"shards": "--workspace", "excluded": EXCLUDING, "matrix": WHOLE, "wasm": "true", "wasi": "true", "fastly": "true"})

    def test_a_read_path_outside_the_crates_mutates_the_whole_workspace(self):
        found = mutated.nightly("g1", changing("crates/herald-core/src/lib.rs", "Cargo.lock"), METADATA)
        self.assertTrue(found.whole)
        self.assertIn("Cargo.lock changed since g1", found.why)

    def test_the_changed_crates_and_their_dependents_are_mutated_the_runtimes_by_their_own_jobs(self):
        found = mutated.nightly("g1", changing("crates/herald-ports/src/lib.rs"), METADATA)
        self.assertFalse(found.whole)
        self.assertEqual(
            found.outputs(),
            {"shards": "--package herald-flows --package herald-ports --package herald-wasm", "excluded": EXCLUDING, "matrix": WHOLE, "wasm": "true",
             "wasi": "false", "fastly": "false"},
        )
        self.assertIn("herald-flows herald-ports herald-wasm", found.why)

    def test_a_crate_the_shards_exclude_whole_is_left_to_its_runtime_job(self):
        metadata = {"workspace_root": ROOT, "packages": [package("herald-wasi"), package("herald-spin")]}
        found = mutated.nightly("g1", changing("crates/herald-wasi/src/lib.rs", "crates/herald-spin/src/lib.rs"), metadata)
        self.assertEqual(found.outputs(), {"shards": "", "excluded": EXCLUDING, "matrix": WHOLE, "wasm": "false", "wasi": "true", "fastly": "false"})

    def test_nothing_changed_mutates_nothing(self):
        found = mutated.nightly("g1", changing(".docs/decisions/0002-y.md"), METADATA)
        self.assertEqual(found.outputs(), {"shards": "", "excluded": EXCLUDING, "matrix": WHOLE, "wasm": "false", "wasi": "false", "fastly": "false"})
        self.assertEqual(found.why, "no crate changed since g1, so nothing is mutated")


class Scope(unittest.TestCase):
    def test_every_run_but_the_nightly_mutates_the_whole_workspace(self):
        for event, schedule, ref in [("schedule", mutated.WEEKLY, mutated.MAIN), ("pull_request", "", "refs/pull/1/merge"),
                                     ("workflow_dispatch", "", mutated.MAIN), ("push", "", "refs/tags/v1.0.0"),
                                     ("", "", ""), ("schedule", "0 0 * * *", mutated.MAIN)]:
            self.assertTrue(mutated.scope(event, schedule, ref).whole, event)
        self.assertEqual(mutated.scope("schedule", mutated.WEEKLY, mutated.MAIN).why,
                         "the weekly run mutates the whole workspace")

    def test_a_pull_request_cuts_its_diff_into_fewer_shards_than_a_whole_run(self):
        found = mutated.scope("pull_request", "", "refs/pull/1/merge")
        self.assertEqual(found.outputs()["matrix"], "[0,1,2,3,4,5,6,7,8,9,10,11]")
        self.assertEqual(found.outputs()["shards"], "--workspace")
        self.assertEqual(mutated.scope("workflow_dispatch", "", mutated.MAIN).outputs()["matrix"], WHOLE)

    def test_a_push_to_main_mutates_nothing(self):
        found = mutated.scope("push", "", mutated.MAIN)
        self.assertEqual(found.outputs(), {"shards": "", "excluded": EXCLUDING, "matrix": WHOLE,
                                           "wasm": "false", "wasi": "false", "fastly": "false"})


class Cutting(unittest.TestCase):
    def test_a_shard_holds_as_many_mutants_as_end_inside_its_minutes_at_the_slowest_rate(self):
        for cut, held in [(mutated.WHOLE_CUT, 645), (mutated.PULL_REQUEST_CUT, 22)]:
            self.assertEqual(cut.shards(1), 1)
            self.assertEqual(cut.shards(held), 1)
            self.assertEqual(cut.shards(held + 1), 2)
            self.assertLessEqual(held * cut.seconds_per_mutant + mutated.SETUP_SECONDS, cut.minutes * 60)
        self.assertEqual(mutated.WHOLE_CUT.shards(18053), 28)
        self.assertEqual(mutated.PULL_REQUEST_CUT.shards(137), 7)

    def test_the_shards_are_capped_and_unknown_mutants_take_the_cap(self):
        for cut in (mutated.WHOLE_CUT, mutated.PULL_REQUEST_CUT):
            self.assertEqual(cut.shards(10**6), cut.most)
            self.assertEqual(cut.shards(None), cut.most)
        self.assertLess(mutated.PULL_REQUEST_CUT.minutes, mutated.WHOLE_CUT.minutes)

    def test_every_shard_aims_well_inside_its_job_s_limit(self):
        workflow = (mutated.ROOT / ".github" / "workflows" / mutated.WORKFLOW).read_text(encoding="utf-8")
        shard = workflow[workflow.index("  mutants-shard:"):workflow.index("  mutants-wasm:")]
        limit = int(re.search(r"timeout-minutes: (\d+)", shard).group(1))
        for cut in (mutated.WHOLE_CUT, mutated.PULL_REQUEST_CUT):
            self.assertLessEqual(cut.minutes * 2, limit)


class Counting(unittest.TestCase):
    def listing(self, mutants: int | None):
        asked = []

        def listed(arguments: list[str]) -> int | None:
            asked.append(arguments)
            return mutants
        return asked, listed

    def test_a_pull_request_lists_its_diff_s_mutants_as_its_shards_mutate_them(self):
        asked, listed = self.listing(137)
        found = mutated.counted(mutated.scope("pull_request", "", "refs/pull/1/merge"), listed, "/t/change.diff")
        self.assertEqual(asked, [["--workspace", *EXCLUDING.split(), "--in-diff", "/t/change.diff"]])
        self.assertEqual(found.outputs()["matrix"], "[0,1,2,3,4,5,6]")
        self.assertTrue(found.why.endswith("; 137 mutants listed, cut into 7 shards"), found.why)

    def test_a_nightly_run_lists_the_packages_it_mutates_whole(self):
        asked, listed = self.listing(700)
        scoped = mutated.Scope(False, frozenset({"herald-core", "herald-wasi"}), "changed")
        found = mutated.counted(scoped, listed, None)
        self.assertEqual(asked, [["--package", "herald-core", *EXCLUDING.split()]])
        self.assertEqual(found.outputs()["matrix"], "[0,1]")

    def test_no_mutant_listed_runs_no_shard_and_leaves_the_runtime_jobs_theirs(self):
        _, listed = self.listing(0)
        found = mutated.counted(mutated.scope("pull_request", "", "refs/pull/1/merge"), listed, "/t/change.diff")
        self.assertEqual(found.outputs()["shards"], "")
        self.assertEqual(found.outputs()["wasm"], "true")
        _, listed = self.listing(11)
        found = mutated.counted(mutated.scope("pull_request", "", "refs/pull/1/merge"), listed, "/t/change.diff")
        self.assertTrue(found.why.endswith("; 11 mutants listed, cut into 1 shard"), found.why)

    def test_mutants_that_cannot_be_listed_take_the_cap(self):
        _, listed = self.listing(None)
        found = mutated.counted(mutated.scope("workflow_dispatch", "", mutated.MAIN), listed, None)
        self.assertEqual(found.outputs()["matrix"], WHOLE)
        self.assertIn("could not be listed, so they are cut into 40 shards", found.why)

    def test_the_mutants_are_counted_from_cargo_mutants_json_listing_or_left_unknown(self):
        def answering(out: str | Exception):
            def run(*command: str) -> str:
                self.assertEqual(command[:4], ("cargo", "mutants", "--list", "--json"))
                if isinstance(out, Exception):
                    raise out
                return out
            return unittest.mock.patch.object(mutated, "run", run)
        for out, mutants in [('[{"name": "a"}, {"name": "b"}]', 2), ("Found 2 mutants", None),
                             (mutated.subprocess.CalledProcessError(1, "cargo"), None), (FileNotFoundError(), None)]:
            with answering(out):
                self.assertEqual(mutated.listed(["--workspace"]), mutants, out)

    def test_a_scope_mutating_nothing_lists_nothing(self):
        asked, listed = self.listing(5)
        found = mutated.counted(mutated.scope("push", "", mutated.MAIN), listed, None)
        self.assertEqual((asked, found.outputs()["shards"]), ([], ""))


FORGE_PLACES = frozenset({".github/CODEOWNERS"})


class Repository(unittest.TestCase):
    def test_the_workflow_is_scheduled_on_the_nightly_and_the_weekly_cron_alone(self):
        workflow = (mutated.ROOT / ".github" / "workflows" / mutated.WORKFLOW).read_text(encoding="utf-8")
        self.assertEqual(re.findall(r'- cron: "([^"]+)"', workflow), [mutated.NIGHTLY, mutated.WEEKLY])

    def test_the_shards_exclude_every_crate_held_unsharded(self):
        workflow = (mutated.ROOT / ".github" / "workflows" / mutated.WORKFLOW).read_text(encoding="utf-8")
        for name in mutated.UNSHARDED:
            self.assertIn(f"crates/{name}/**", mutated.EXCLUDED)
        shard = workflow[workflow.index("  mutants-shard:"):workflow.index("  mutants-wasm:")]
        self.assertIn('--sharding round-robin "${excluded[@]}"', shard)
        self.assertNotIn("--exclude", shard)

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
