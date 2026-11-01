#!/usr/bin/env python3
"""What the release workflow decides: the next version, its changelog section and its tag.

Usage:
  release.py next       the version the commits since the last tag make, or
                        VERSION where given; writes version=, line= and
                        prerelease= lines to $GITHUB_OUTPUT
  release.py prepend    puts the section on stdin at the top of CHANGELOG.md
  release.py notes      prints TAG's section of CHANGELOG.md
  release.py check-tag  refuses TAG where it is not a version, and writes
                        line= and prerelease= to $GITHUB_OUTPUT

Each refuses with exit code 2 and says why (ADR-0012).

Two halves: the functions below `main` read git, the environment and files,
and the ones above decide from what they read. The deciding half is what
test_release.py tests.
"""
import os
import re
import subprocess
import sys

VERSION = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?$")

# `type(scope)!: subject`, where the `!` says the change breaks.
SUBJECT = re.compile(r"^(?P<type>[a-z]+)(\([^)]*\))?(?P<breaking>!)?: ")

BREAKING_FOOTER = re.compile(r"^BREAKING[ -]CHANGE: ", re.MULTILINE)

CHANGELOG = "CHANGELOG.md"

LEVELS = ("patch", "minor", "major")


class Refused(Exception):
    """Why the release cannot go ahead, said to the maintainer reading the log."""


def parse(version: str) -> tuple[int, int, int, str]:
    """A version's numbers and its pre-release suffix, empty for a release."""
    matched = VERSION.match(version)
    if matched is None:
        raise Refused(f"{version!r} is not a version; it is X.Y.Z, or X.Y.Z-pre for a pre-release.")
    return int(matched[1]), int(matched[2]), int(matched[3]), matched[4] or ""


def spelled(version: str) -> str:
    major, minor, patch, pre = parse(version)
    return f"{major}.{minor}.{patch}{pre}"


def precedes(earlier: str, later: str) -> bool:
    """Whether `earlier` comes before `later`: a pre-release comes before its release."""
    first, second = parse(earlier), parse(later)
    if first[:3] != second[:3]:
        return first[:3] < second[:3]
    return bool(first[3]) and (not second[3] or first[3] < second[3])


def level(message: str) -> str | None:
    """What one commit asks of the version: a major, a minor, a patch or nothing."""
    subject = SUBJECT.match(message)
    if subject is None:
        return None
    if subject["breaking"] or BREAKING_FOOTER.search(message):
        return "major"
    return {"feat": "minor", "fix": "patch", "perf": "patch"}.get(subject["type"])


def next_version(last: str | None, messages: list[str], given: str) -> str:
    """The version to release: the one given, or the last one moved by the commits since.

    Below 1.0.0 a breaking change is a minor: 1.0.0 is given by hand, as is
    every pre-release.
    """
    if given:
        if last is not None and not precedes(last, given):
            raise Refused(f"{given} is not after {last}.")
        return spelled(given)
    if last is None:
        raise Refused("No release is tagged yet: give the first version by hand.")
    asked = [found for found in map(level, messages) if found is not None]
    if not asked:
        raise Refused(f"Nothing to release: no feat, fix or perf since {last}.")
    major, minor, patch, pre = parse(last)
    if pre:
        return f"{major}.{minor}.{patch}"
    highest = max(asked, key=LEVELS.index)
    if highest == "major" and major > 0:
        return f"{major + 1}.0.0"
    if highest == "patch":
        return f"{major}.{minor}.{patch + 1}"
    return f"{major}.{minor + 1}.0"


def prepend(changelog: str, section: str) -> str:
    """The changelog with a version's section above every earlier one, below the header."""
    lines = changelog.splitlines(keepends=True)
    first = next((index for index, line in enumerate(lines) if line.startswith("## ")), len(lines))
    head = "".join(lines[:first]).rstrip("\n") + "\n\n"
    rest = "".join(lines[first:])
    return head + section.strip("\n") + "\n" + ("\n" + rest if rest else "")


def notes(changelog: str, version: str) -> str:
    """A version's section of the changelog, without its heading: the release's notes."""
    wanted = spelled(version)
    heading = re.compile(rf"^## \[{re.escape(wanted)}\].*$", re.MULTILINE)
    found = heading.search(changelog)
    if found is None:
        raise Refused(f"{CHANGELOG} has no section for {wanted}: merge the release pull request first.")
    following = re.compile(r"^## ", re.MULTILINE).search(changelog, found.end())
    body = changelog[found.end() : following.start() if following else len(changelog)].strip()
    if not body:
        raise Refused(f"{CHANGELOG}'s section for {wanted} is empty.")
    return body + "\n"


def line(version: str) -> str:
    """A release's line, whose tag (`v0.1`) each release of the line moves; none for a pre-release."""
    major, minor, _, pre = parse(version)
    if pre:
        return ""
    return f"{major}.{minor}" if major == 0 else f"{major}"


def outputs(values: dict[str, str]) -> str:
    """Lines for $GITHUB_OUTPUT, refusing a value that would start another line."""
    for name, value in values.items():
        if "\n" in value or "\r" in value:
            raise Refused(f"{name} holds a line break.")
    return "".join(f"{name}={value}\n" for name, value in values.items())


def main(argv: list[str]) -> int:
    commands = {"next": _next, "prepend": _prepend, "notes": _notes, "check-tag": _check_tag}
    if len(argv) != 2 or argv[1] not in commands:
        print(__doc__, file=sys.stderr)
        return 2
    try:
        commands[argv[1]]()
    except Refused as refused:
        print(f"::error::{refused}", file=sys.stderr)
        return 2
    return 0


def _released(version: str) -> dict[str, str]:
    return {"line": line(version), "prerelease": "true" if parse(version)[3] else "false"}


def _next() -> None:
    last = _last_tag()
    version = next_version(last, _messages(last), os.environ.get("VERSION", ""))
    _write_outputs({"version": version, **_released(version)})


def _prepend() -> None:
    _write_file(CHANGELOG, prepend(_read_file(CHANGELOG), sys.stdin.read()))


def _notes() -> None:
    sys.stdout.write(notes(_read_file(CHANGELOG), os.environ["TAG"]))


def _check_tag() -> None:
    tag = os.environ["TAG"]
    if not tag.startswith("v"):
        raise Refused(f"{tag} is not a release tag; it is vX.Y.Z, or vX.Y.Z-pre for a pre-release.")
    _write_outputs(_released(tag))


def _write_outputs(values: dict[str, str]) -> None:
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as file:
        file.write(outputs(values))


def _last_tag() -> str | None:
    found = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*.[0-9]*.[0-9]*"],
        capture_output=True,
        text=True,
        check=False,
    )
    return found.stdout.strip() if found.returncode == 0 else None


def _messages(last: str | None) -> list[str]:
    since = [f"{last}..HEAD"] if last else ["HEAD"]
    log = subprocess.run(
        ["git", "log", "--first-parent", "--format=%B%x00", *since],
        capture_output=True,
        text=True,
        check=True,
    )
    return [message.strip() for message in log.stdout.split("\0") if message.strip()]


def _read_file(path: str) -> str:
    try:
        with open(path, encoding="utf-8") as file:
            return file.read()
    except FileNotFoundError:
        return ""


def _write_file(path: str, text: str) -> None:
    with open(path, "w", encoding="utf-8") as file:
        file.write(text)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
