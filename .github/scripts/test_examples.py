"""examples.py's pins of herald's image: python3 -m unittest discover .github/scripts"""
import io
import pathlib
import shutil
import subprocess
import tempfile
import unittest
import unittest.mock

import examples
import release

FIRST = "sha256:" + "0123456789abcdef" * 4
SECOND = "sha256:" + "fedcba9876543210" * 4


def tracked() -> dict[str, str]:
    """Each text file git tracks in the repository, by its path."""
    listed = subprocess.run(
        ["git", "ls-files", "-z"], cwd=examples.ROOT, check=True, capture_output=True, text=True
    ).stdout
    texts = {}
    for path in filter(None, listed.split("\0")):
        try:
            texts[path] = (examples.ROOT / path).read_text(encoding="utf-8")
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
    return texts


class Examples(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for path in (*examples.PINNED, examples.KUSTOMIZATION):
            (self.root / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(examples.ROOT / path, self.root / path)

    def read(self, path: str) -> str:
        return (self.root / path).read_text(encoding="utf-8")

    def test_every_file_that_pulls_the_image_is_one_it_pins(self):
        pulling = {path for path, text in tracked().items() if examples.pulls(text)}
        self.assertEqual(pulling, set(examples.PINNED))
        self.assertEqual(len(examples.ENTRY.findall(self.read(examples.KUSTOMIZATION))), 1)

    def test_a_release_pins_each_pull_and_the_kustomization_and_a_second_run_changes_nothing(self):
        pulled = {path: len(examples.pulls(self.read(path))) for path in examples.PINNED}
        named = {path: self.read(path).count(examples.IMAGE) for path in examples.PINNED}
        self.assertEqual(examples.pin(self.root, "v1.2.3", FIRST), [*examples.PINNED, examples.KUSTOMIZATION])
        for path in examples.PINNED:
            self.assertEqual(examples.pulls(self.read(path)), [f"{examples.IMAGE}:1.2.3@{FIRST}"] * pulled[path])
            self.assertEqual(self.read(path).count(examples.IMAGE), named[path])
        self.assertIn(
            f'  - name: {examples.IMAGE}\n    newTag: "1.2.3"\n    digest: {FIRST}\n', self.read(examples.KUSTOMIZATION)
        )
        pinned = {path: self.read(path) for path in (*examples.PINNED, examples.KUSTOMIZATION)}
        self.assertEqual(examples.pin(self.root, "1.2.3", FIRST), [])
        self.assertEqual({path: self.read(path) for path in pinned}, pinned)

    def test_the_next_release_replaces_the_pins_of_the_last(self):
        examples.pin(self.root, "1.2.3", FIRST)
        self.assertEqual(examples.pin(self.root, "1.3.0-rc.1", SECOND), [*examples.PINNED, examples.KUSTOMIZATION])
        for path in (*examples.PINNED, examples.KUSTOMIZATION):
            self.assertNotIn(FIRST, self.read(path))
            self.assertIn(SECOND, self.read(path))
        self.assertIn(f'newTag: "1.3.0-rc.1"\n    digest: {SECOND}\n', self.read(examples.KUSTOMIZATION))

    def test_the_bare_name_a_placeholder_and_other_images_are_left(self):
        text = (
            f"`{examples.IMAGE}` runs\n{examples.IMAGE}:<version>\n{examples.IMAGE}:VERSION@DIGEST\n{examples.IMAGE}-component:1\n"
            f"{examples.IMAGE}/v1.0.0\nghcr.io/nightworksio/charts/herald:1\nimage: {examples.IMAGE}:0\n"
        )
        self.assertEqual(examples.pulls(text), [f"{examples.IMAGE}:0"])
        self.assertEqual(
            examples.rewritten("deploy/README.md", text, "1.2.3", FIRST),
            text.replace(f"image: {examples.IMAGE}:0", f"image: {examples.IMAGE}:1.2.3@{FIRST}"),
        )

    def test_a_version_or_a_digest_it_cannot_take_is_refused_and_nothing_is_written(self):
        before = {path: self.read(path) for path in (*examples.PINNED, examples.KUSTOMIZATION)}
        for version, digest in [
            ("1.2", FIRST),
            ("1.2.3", "sha256:abc"),
            ("1.2.3", FIRST.upper()),
            ("1.2.3", "sha512:" + "0" * 128),
        ]:
            with self.subTest(version=version, digest=digest), self.assertRaises(release.Refused):
                examples.pin(self.root, version, digest)
        self.assertEqual({path: self.read(path) for path in before}, before)

    def test_a_pinned_file_that_pulls_nothing_or_a_kustomization_with_no_entry_is_refused(self):
        with self.assertRaisesRegex(release.Refused, "pulls ghcr.io/nightworksio/herald nowhere"):
            examples.rewritten("deploy/README.md", f"`{examples.IMAGE}`\n", "1.2.3", FIRST)
        with self.assertRaisesRegex(release.Refused, "holds 0 entries"):
            examples.rewritten(examples.KUSTOMIZATION, "images: []\n", "1.2.3", FIRST)

    def test_main_prints_what_it_changed_or_why_it_refused(self):
        out, err = io.StringIO(), io.StringIO()
        with unittest.mock.patch.object(examples, "ROOT", self.root), unittest.mock.patch("sys.stdout", out):
            self.assertEqual(examples.main(["1.2.3", FIRST]), 0)
        self.assertEqual(out.getvalue().splitlines(), [*examples.PINNED, examples.KUSTOMIZATION])
        with unittest.mock.patch.object(examples, "ROOT", self.root), unittest.mock.patch("sys.stderr", err):
            self.assertEqual(examples.main(["1.2.3", "sha256:abc"]), 2)
            self.assertEqual(examples.main(["1.2.3"]), 2)
        self.assertIn("is not an image digest", err.getvalue())
        self.assertIn("usage: examples.py VERSION DIGEST", err.getvalue())


if __name__ == "__main__":
    unittest.main()
