#!/usr/bin/env bash
# Installs one CI tool into ~/.cargo/bin from its release, pinned by version and
# by the SHA-256 of the asset. Where a release lists checksums, the pin is the
# one it lists; where it lists none, the one this repository first downloaded.
set -euo pipefail

tool="${1:?usage: install-tool.sh <cargo-nextest|cargo-llvm-cov|cargo-deny|cargo-audit|cargo-mutants|gitleaks>}"

case "$tool" in
  cargo-nextest)
    version="0.9.150"
    url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${version}/cargo-nextest-${version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256="b0959bc64232974224cf9fb22c915845398c0e8668fa7831eb5b43fb1eef781f"
    member="cargo-nextest"
    ;;
  cargo-llvm-cov)
    version="0.9.1"
    url="https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${version}/cargo-llvm-cov-x86_64-unknown-linux-gnu.tar.gz"
    sha256="b3f68e625481fed9b16444174f3fa5ebcdbde4a1878803a35eabe2dcefcdc41a"
    member="cargo-llvm-cov"
    ;;
  cargo-deny)
    version="0.20.2"
    url="https://github.com/EmbarkStudios/cargo-deny/releases/download/${version}/cargo-deny-${version}-x86_64-unknown-linux-musl.tar.gz"
    sha256="9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f"
    member="cargo-deny-${version}-x86_64-unknown-linux-musl/cargo-deny"
    ;;
  cargo-audit)
    version="0.22.2"
    url="https://github.com/rustsec/rustsec/releases/download/cargo-audit%2Fv${version}/cargo-audit-x86_64-unknown-linux-gnu-v${version}.tgz"
    sha256="ab28a1bdb54db4d5d8ad5981cf1f959410370b3d28250dbd35f6a44248620e39"
    member="cargo-audit-x86_64-unknown-linux-gnu-v${version}/cargo-audit"
    ;;
  cargo-mutants)
    version="27.1.0"
    url="https://github.com/sourcefrog/cargo-mutants/releases/download/v${version}/cargo-mutants-x86_64-unknown-linux-gnu.tar.gz"
    sha256="dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e"
    member="cargo-mutants"
    ;;
  gitleaks)
    version="8.30.1"
    url="https://github.com/gitleaks/gitleaks/releases/download/v${version}/gitleaks_${version}_linux_x64.tar.gz"
    sha256="551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"
    member="gitleaks"
    ;;
  *)
    echo "no pin for ${tool}" >&2
    exit 2
    ;;
esac

archive="${RUNNER_TEMP:-/tmp}/${tool}.tar.gz"
curl -sSfL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 --retry-all-errors -o "$archive" "$url"
sha256sum --check --strict <<<"${sha256}  ${archive}"
mkdir -p "${HOME}/.cargo/bin"
tar xzf "$archive" -C "${HOME}/.cargo/bin" --strip-components="$(tr -cd '/' <<<"$member" | wc -c)" "$member"
rm -f "$archive"
echo "${tool} ${version}"
