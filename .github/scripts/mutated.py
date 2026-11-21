"""What a ci run mutates (ADR-0070). A pull request, a run started by hand, a
release's gate and the weekly run mutate the whole workspace, a pull request's
narrowed to its diff by its shards. The nightly run mutates the crates changed
since the last green scheduled run on main, and every crate depending on one.
Where that run cannot be found, or a change outside the crates may reach a
build or a test, the nightly run mutates the whole workspace too. It writes
A push to main mutates nothing. It runs on every event, and writes
the scope to $GITHUB_OUTPUT: `shards` (the cargo-mutants arguments naming
what the shards mutate, empty for nothing), and `wasm`, `wasi` and `fastly`
(whether the runtime jobs mutate those crates)."""
import json
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
UNREAD_DIRECTORIES = (".docs/decisions/", ".github/", "fuzz/")
READ_INSIDE = (".github/workflows/ci.yml", ".github/scripts/install-tool.sh", ".github/dogfood")
UNREAD_FILES = frozenset({
    ".editorconfig", ".gitattributes", ".gitignore", ".gitleaks.toml",
    ".markdownlint.jsonc", ".sonarcloud.properties", "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md", "Containerfile", "LICENSE-APACHE", "LICENSE-MIT",
    "SECURITY.md", "action.yml", "cliff.toml", "lychee.toml", "typos.toml",
})


class Scope(NamedTuple):
    whole: bool
    packages: frozenset[str]
    why: str

    def outputs(self) -> dict[str, str]:
        """The job outputs naming it."""
        if self.whole:
            shards = "--workspace"
        else:
            shards = " ".join(f"--package {name}" for name in sorted(self.packages - UNSHARDED))
        reached = {name.removeprefix("herald-"): self.whole or name in self.packages for name in RUNTIMES}
        return {"shards": shards} | {name: str(value).lower() for name, value in reached.items()}


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


if __name__ == "__main__":
    found = scope(os.environ.get("EVENT", ""), os.environ.get("SCHEDULE", ""), os.environ.get("REF", ""))
    print(found.why)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in found.outputs().items():
            output.write(f"{name}={value}\n")
