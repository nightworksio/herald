#!/usr/bin/env python3
"""Pins every example that pulls herald's server image to one release, by
its version and its digest (ADR-0009).

Usage:
  examples.py VERSION DIGEST  rewrites each example under the repository's
                              root and prints the path of each it changed

Each reference that pulls the image, `ghcr.io/nightworksio/herald:<tag>`
with or without a digest, becomes `ghcr.io/nightworksio/herald:VERSION@DIGEST`;
the kustomization's entry for the image takes VERSION as its `newTag` and
DIGEST as its `digest`. The image's bare name is left as it is. A second
run with the same arguments changes nothing. It refuses with exit code 2
and says why.
"""
import pathlib
import re
import sys

import release

ROOT = pathlib.Path(__file__).resolve().parents[2]
IMAGE = "ghcr.io/nightworksio/herald"
DIGEST = r"sha256:[0-9a-f]{64}"
REFERENCE = re.compile(re.escape(IMAGE) + rf"(?P<pin>(?::[0-9A-Za-z_][0-9A-Za-z_.-]*)?(?:@{DIGEST})?)(?![-/@\w])")
ENTRY = re.compile(
    rf'^(?P<indent> *)- name: {re.escape(IMAGE)}\n(?P<inner> +)newTag: "[^"\n]*"\n(?:(?P=inner)digest: {DIGEST}\n)?',
    re.MULTILINE,
)
PINNED = ("deploy/README.md", "deploy/compose/compose.yaml", "deploy/kubernetes/deployment.yaml")
KUSTOMIZATION = "deploy/kubernetes/kustomization.yaml"


def pulls(text: str) -> list[str]:
    """Each reference in `text` that pulls the image, as written."""
    return [found[0] for found in REFERENCE.finditer(text) if found["pin"]]


def rewritten(path: str, text: str, version: str, digest: str) -> str:
    """`text`, the file at `path`, with each pull of the image pinned to
    `version` and `digest`."""
    if path == KUSTOMIZATION:
        pinned, entries = ENTRY.subn(
            lambda found: f'{found["indent"]}- name: {IMAGE}\n{found["inner"]}newTag: "{version}"\n{found["inner"]}digest: {digest}\n',
            text,
        )
        if entries != 1:
            raise release.Refused(f"{path} holds {entries} entries for {IMAGE} with a newTag, where one is pinned.")
        return pinned
    if not pulls(text):
        raise release.Refused(f"{path} pulls {IMAGE} nowhere; drop it from examples.py's PINNED.")
    return REFERENCE.sub(lambda found: f"{IMAGE}:{version}@{digest}" if found["pin"] else found[0], text)


def pin(root: pathlib.Path, version: str, digest: str) -> list[str]:
    """Pins each example under `root` to `version` and `digest`; the paths
    of those it changed."""
    version = release.spelled(version)
    if not re.fullmatch(DIGEST, digest):
        raise release.Refused(f"{digest!r} is not an image digest; it is sha256: and 64 lowercase hex digits.")
    changed = []
    for path in (*PINNED, KUSTOMIZATION):
        file = root / path
        text = file.read_text(encoding="utf-8")
        pinned = rewritten(path, text, version, digest)
        if pinned != text:
            file.write_text(pinned, encoding="utf-8")
            changed.append(path)
    return changed


def main(arguments: list[str]) -> int:
    if len(arguments) != 2:
        print("usage: examples.py VERSION DIGEST", file=sys.stderr)
        return 2
    try:
        changed = pin(ROOT, *arguments)
    except release.Refused as refused:
        print(refused, file=sys.stderr)
        return 2
    for path in changed:
        print(path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
