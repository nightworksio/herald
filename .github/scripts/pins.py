"""Names every hand pin with a newer release: the tools install-tool.sh
installs, the images pinned by digest, and the musl-tools package the release
links against. A tool's release counts once it is a week old, the cooldown
dependabot.yml gives every ecosystem. It exits 1 while any pin is behind."""
import pathlib
import re
import subprocess
import sys
import urllib.parse
from typing import NamedTuple

ROOT = pathlib.Path(__file__).resolve().parents[2]
HOSTS = {"dl.k8s.io": "kubernetes/kubernetes", "get.helm.sh": "helm/helm"}
DIGESTED = re.compile(r"([\w./-]+(?::[\w.-]+)?)@(sha256:[0-9a-f]{64})")
MUSL = re.compile(r"musl-tools=([\w.~+-]+)")
NUMBER = r"(\d+(?:\.\d+)+)"
COOLDOWN_SECONDS = 7 * 24 * 60 * 60


class Pin(NamedTuple):
    name: str
    version: str
    repository: str
    shape: str


def tools(script: str) -> list[Pin]:
    """Each tool's pinned version, the repository it is released from, and the
    shape of that repository's tags, `{}` standing for the version."""
    pins: dict[tuple[str, str], Pin] = {}
    for block in re.finditer(r"^ {2}([\w| -]+)\)\n(.*?)\n {4};;", script, re.M | re.S):
        version = re.search(r'version="([^"]+)"', block.group(2))
        url = re.search(r'url="([^"]+)"', block.group(2))
        if not (version and url):
            continue
        parsed = urllib.parse.urlparse(url.group(1))
        parts = parsed.path.split("/")
        if parsed.netloc == "github.com":
            repository = "/".join(parts[1:3])
            shape = urllib.parse.unquote(parts[5]).replace("${version}", "{}")
        else:
            repository = HOSTS[parsed.netloc]
            shape = "v{}"
        name = block.group(1).split("|")[0].strip()
        pins.setdefault((repository, shape), Pin(name, version.group(1), repository, shape))
    return list(pins.values())


def numbers(version: str) -> tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def newer(pin: Pin, tags: list[str]) -> str | None:
    """The newest version among `tags` of the pin's shape, where it is newer."""
    shaped = re.compile(re.escape(pin.shape).replace(re.escape("{}"), NUMBER))
    versions = [match.group(1) for tag in tags if (match := shaped.fullmatch(tag))]
    newest = max(versions, key=numbers, default=pin.version)
    return newest if numbers(newest) > numbers(pin.version) else None


def images(texts: list[str]) -> set[tuple[str, str]]:
    """Every image pinned by digest, with the digest."""
    return {match.groups() for text in texts for match in DIGESTED.finditer(text)}


def run(*command: str) -> str:
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def behind() -> list[str]:
    found = []
    for pin in tools((ROOT / ".github/scripts/install-tool.sh").read_text()):
        tags = run("gh", "api", f"repos/{pin.repository}/releases?per_page=100", "--jq",
                   ".[] | select(.draft or .prerelease | not)"
                   f" | select((.published_at | fromdateiso8601) < now - {COOLDOWN_SECONDS})"
                   " | .tag_name").split()
        if version := newer(pin, tags):
            found.append(f"{pin.name} {pin.version} has {version}")
    tracked = run("git", "-C", str(ROOT), "ls-files").split()
    texts = [(ROOT / path).read_text() for path in tracked
             if path.startswith((".github/workflows/", "deploy/")) or path == "Containerfile"]
    for image, digest in sorted(images(texts)):
        current = run("docker", "buildx", "imagetools", "inspect", image,
                      "--format", "{{json .Manifest.Digest}}").strip().strip('"')
        if current != digest:
            found.append(f"{image} {digest} has {current}")
    candidate = re.search(r"Candidate: (\S+)", run("apt-cache", "policy", "musl-tools"))
    for pinned in sorted({match for text in texts for match in MUSL.findall(text)}):
        if candidate and candidate.group(1) != pinned:
            found.append(f"musl-tools {pinned} has {candidate.group(1)}")
    return found


if __name__ == "__main__":
    lines = behind()
    print("\n".join(lines) if lines else "Every hand pin is current.")
    sys.exit(1 if lines else 0)
