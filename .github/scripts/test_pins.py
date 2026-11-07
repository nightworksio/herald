"""pins.py's reading of the pins: python3 -m unittest discover .github/scripts"""
import unittest

import pins

SCRIPT = """case "$tool" in
  cargo-audit)
    version="0.22.2"
    url="https://github.com/rustsec/rustsec/releases/download/cargo-audit%2Fv${version}/cargo-audit.tgz"
    member="cargo-audit"
    ;;
  wasm-bindgen | wasm-bindgen-test-runner)
    version="0.2.130"
    url="https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/wasm-bindgen.tar.gz"
    member="wasm-bindgen"
    ;;
  helm)
    version="4.3.1"
    url="https://get.helm.sh/helm-v${version}-linux-amd64.tar.gz"
    member="linux-amd64/helm"
    ;;
  *)
    exit 2
    ;;
esac
"""


class Tools(unittest.TestCase):
    def test_each_tool_is_read_with_its_repository_and_tag_shape(self):
        self.assertEqual(pins.tools(SCRIPT), [
            pins.Pin("cargo-audit", "0.22.2", "rustsec/rustsec", "cargo-audit/v{}"),
            pins.Pin("wasm-bindgen", "0.2.130", "wasm-bindgen/wasm-bindgen", "{}"),
            pins.Pin("helm", "4.3.1", "helm/helm", "v{}"),
        ])


class Newer(unittest.TestCase):
    PIN = pins.Pin("cargo-audit", "0.22.2", "rustsec/rustsec", "cargo-audit/v{}")

    def test_the_newest_tag_of_the_pin_s_shape_is_named(self):
        tags = ["rustsec/v0.31.0", "cargo-audit/v0.22.10", "cargo-audit/v0.22.9"]
        self.assertEqual(pins.newer(self.PIN, tags), "0.22.10")

    def test_nothing_is_named_where_the_pin_is_newest(self):
        self.assertIsNone(pins.newer(self.PIN, ["cargo-audit/v0.22.2", "cargo-audit/v0.9.0"]))
        self.assertIsNone(pins.newer(self.PIN, ["v1.0.0"]))


class Images(unittest.TestCase):
    def test_each_digest_pin_is_read_once(self):
        digest = "sha256:" + "a" * 64
        text = f"image: postgres:17-alpine@{digest}\nFROM gcr.io/distroless/static-debian12@{digest}\n"
        self.assertEqual(pins.images([text, text]), {
            ("postgres:17-alpine", digest),
            ("gcr.io/distroless/static-debian12", digest),
        })


if __name__ == "__main__":
    unittest.main()
