"""What a ci run mutates (ADR-0070). A pull request, a run started by hand, a
release's gate and the weekly run mutate the whole workspace, a pull request's
narrowed to its diff by its shards. The nightly run mutates the crates changed
since the last green scheduled run on main, and every crate depending on one.
Where that run cannot be found, or a change outside the crates may reach a
build or a test, the nightly run mutates the whole workspace too. A push to
main mutates nothing. It runs on every event, and writes the scope to
$GITHUB_OUTPUT: `shards` (the cargo-mutants arguments naming what the shards
mutate, empty for nothing), `excluded` (the cargo-mutants arguments naming
what they leave to the runtime jobs), `matrix` (the shards' indices, as
JSON), and `wasm`, `wasi` and `fastly` (whether the runtime jobs mutate those
crates). The shards are as many as the mutants listed take to keep each well
inside its job's limit, at most a cap; where the mutants cannot be listed,
the cap."""
import json
import math
import os
import pathlib
import subprocess
from typing import Callable, NamedTuple

ROOT = pathlib.Path(__file__).resolve().parents[2]
NIGHTLY = "17 2 * * 0-5"
WEEKLY = "17 2 * * 6"
WORKFLOW = "ci.yml"
MAIN = "refs/heads/main"
RUNS_READ = 30
RUNTIMES = ("herald-wasm", "herald-wasi", "herald-fastly")
UNSHARDED = frozenset({"herald-wasi", "herald-fastly", "herald-spin"})
EXCLUDED = ("crates/herald-wasm/src/js/**",) + tuple(f"crates/{name}/**" for name in sorted(UNSHARDED))
EXCLUDING = tuple(argument for glob in EXCLUDED for argument in ("--exclude", glob))
SETUP_SECONDS = 240


class Cut(NamedTuple):
    """How a run's mutants are cut into shards: the seconds a mutant takes
    at the slowest seen, the minutes a shard aims to end in, and the most
    shards."""
    seconds_per_mutant: int
    minutes: int
    most: int

    def shards(self, mutants: int | None) -> int:
        """The shards `mutants` are cut into: the most where they are not
        known, else as many as keep each inside its minutes, at least one."""
        if mutants is None:
            return self.most
        held = (self.minutes * 60 - SETUP_SECONDS) // self.seconds_per_mutant
        return max(1, min(self.most, math.ceil(mutants / held)))


WHOLE_CUT = Cut(seconds_per_mutant=8, minutes=90, most=40)
PULL_REQUEST_CUT = Cut(seconds_per_mutant=30, minutes=15, most=12)
UNREAD_DIRECTORIES = (".docs/decisions/", ".github/", "fuzz/")
READ_INSIDE = (".github/workflows/ci.yml", ".github/scripts/install-tool.sh", ".github/dogfood")
UNREAD_FILES = frozenset({
    ".editorconfig", ".gitattributes", ".gitignore", ".gitleaks.toml",
    ".markdownlint.jsonc", ".sonarcloud.properties", "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md", "Containerfile", "LICENSE-APACHE", "LICENSE-MIT",
    "SECURITY.md", "action.yml", "cliff.toml", "lychee.toml", "osv-scanner.toml",
    "typos.toml",
})


class Scope(NamedTuple):
    whole: bool
    packages: frozenset[str]
    why: str
    cut: Cut = WHOLE_CUT
    mutants: int | None = None

    def mutated(self) -> list[str]:
        """The cargo-mutants arguments naming what the shards mutate, none
        for nothing."""
        if self.whole:
            return ["--workspace"]
        return [argument for name in sorted(self.packages - UNSHARDED) for argument in ("--package", name)]

    def outputs(self) -> dict[str, str]:
        """The job outputs naming it."""
        shards = "" if self.mutants == 0 else " ".join(self.mutated())
        reached = {name.removeprefix("herald-"): self.whole or name in self.packages for name in RUNTIMES}
        matrix = json.dumps(list(range(self.cut.shards(self.mutants))), separators=(",", ":"))
        return {"shards": shards, "excluded": " ".join(EXCLUDING), "matrix": matrix} | {
            name: str(value).lower() for name, value in reached.items()
        }


def whole(why: str) -> Scope:
    return Scope(True, frozenset(), why)


def members(metadata: dict) -> dict[str, str]:
    """Each workspace member's name, by its directory relative to the root."""
    root = pathlib.PurePosixPath(metadata["workspace_root"])
    return {
        pathlib.PurePosixPath(package["manifest_path"]).parent.relative_to(root).as_posix() + "/": package["name"]
        for package in metadata["packages"]
    }


def unread(path: str) -> bool:
    """Whether no build and no test reads `path`."""
    if path.startswith(READ_INSIDE):
        return False
    return path in UNREAD_FILES or path.startswith(UNREAD_DIRECTORIES)


def changed(paths: list[str], crates: dict[str, str]) -> frozenset[str] | str:
    """The crates `paths` change, or the first path that may reach a build or
    a test outside every crate."""
    found = set()
    for path in paths:
        owner = next((name for directory, name in crates.items() if path.startswith(directory)), None)
        if owner is not None:
            found.add(owner)
        elif not unread(path):
            return path
    return frozenset(found)


def dependents(metadata: dict, crates: frozenset[str]) -> frozenset[str]:
    """`crates` and every workspace member depending on one of them, by any
    kind of dependency, directly or through another."""
    named = members(metadata)
    root = pathlib.PurePosixPath(metadata["workspace_root"])
    depends = {
        package["name"]: {
            named.get(pathlib.PurePosixPath(dependency["path"]).relative_to(root).as_posix() + "/")
            for dependency in package["dependencies"]
            if dependency.get("path")
        }
        for package in metadata["packages"]
    }
    reached = set(crates)
    grown = True
    while grown:
        more = {name for name, on in depends.items() if on & reached} - reached
        reached |= more
        grown = bool(more)
    return frozenset(reached)


def last_green(runs: dict, present: Callable[[str], bool]) -> str | None:
    """The head of the newest green scheduled run `runs` lists whose commit
    this clone holds."""
    return next((run["head_sha"] for run in runs.get("workflow_runs", []) if present(run["head_sha"])), None)


def nightly(green: str | None, paths: Callable[[str], list[str]], metadata: dict) -> Scope:
    """The nightly run's scope, from `green`, the head of the last green
    scheduled run, the paths changed since a commit, and the workspace."""
    if green is None:
        return whole("no green scheduled run on main is found, so the whole workspace is mutated")
    found = changed(paths(green), members(metadata))
    if isinstance(found, str):
        return whole(f"{found} changed since {green} and may reach a build or a test, so the whole workspace is mutated")
    reached = dependents(metadata, found)
    if not reached:
        return Scope(False, reached, f"no crate changed since {green}, so nothing is mutated")
    return Scope(False, reached, f"changed since {green}, with their dependents: {' '.join(sorted(reached))}")


def run(*command: str) -> str:
    return subprocess.run(command, cwd=ROOT, check=True, capture_output=True, text=True).stdout


def present(sha: str) -> bool:
    return subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"], cwd=ROOT, capture_output=True).returncode == 0


def scope(event: str, schedule: str, ref: str) -> Scope:
    if event == "push" and ref == MAIN:
        return Scope(False, frozenset(), "a push to main mutates nothing: its pull request mutated its diff, and the next scheduled run mutates it again")
    if event == "schedule" and schedule == WEEKLY:
        return whole("the weekly run mutates the whole workspace")
    if event == "pull_request":
        return whole("a pull_request run mutates the whole workspace narrowed to its diff")._replace(cut=PULL_REQUEST_CUT)
    if event != "schedule" or schedule != NIGHTLY:
        return whole(f"a {event or 'called'} run mutates the whole workspace, a pull request's narrowed to its diff")
    repository = os.environ["GITHUB_REPOSITORY"]
    listed = f"repos/{repository}/actions/workflows/{WORKFLOW}/runs?event=schedule&status=success&branch=main&per_page={RUNS_READ}"
    try:
        runs = json.loads(run("gh", "api", listed))
    except (subprocess.CalledProcessError, json.JSONDecodeError) as error:
        return whole(f"the green scheduled runs could not be listed ({error}), so the whole workspace is mutated")
    metadata = json.loads(run("cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"))
    return nightly(last_green(runs, present), lambda green: run("git", "diff", "--name-only", green, "HEAD").splitlines(), metadata)


def listed(arguments: list[str]) -> int | None:
    """How many mutants cargo-mutants lists with `arguments`, or none where
    it cannot list them."""
    try:
        return len(json.loads(run("cargo", "mutants", "--list", "--json", *arguments)))
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError, TypeError):
        return None


def counted(found: Scope, listing: Callable[[list[str]], int | None], diff: str | None) -> Scope:
    """`found` with the mutants its shards mutate counted by `listing`, a
    pull request's narrowed to the diff at the path `diff`."""
    mutated = found.mutated()
    if not mutated:
        return found
    narrowed = ["--in-diff", diff] if diff is not None else []
    mutants = listing([*mutated, *EXCLUDING, *narrowed])
    shards = found.cut.shards(mutants)
    if mutants is None:
        return found._replace(why=f"{found.why}; the mutants could not be listed, so they are cut into {shards} shards")
    return found._replace(mutants=mutants, why=f"{found.why}; {mutants} mutants listed, cut into {shards} shards")


def changes(base: str, into: pathlib.Path) -> str:
    """The path `into`, holding the diff since the merge base of `base`."""
    merged = run("git", "merge-base", base, "HEAD").strip()
    into.write_text(run("git", "diff", merged, "HEAD"), encoding="utf-8")
    return str(into)


if __name__ == "__main__":
    event = os.environ.get("EVENT", "")
    found = scope(event, os.environ.get("SCHEDULE", ""), os.environ.get("REF", ""))
    diff = changes(os.environ["BASE"], pathlib.Path(os.environ["RUNNER_TEMP"]) / "change.diff") if event == "pull_request" else None
    found = counted(found, listed, diff)
    print(found.why)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in found.outputs().items():
            output.write(f"{name}={value}\n")
