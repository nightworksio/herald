#!/usr/bin/env bash
# Updates apt's lists and installs the pinned packages it is given, each
# attempt retried and timed out. Where the runner's mirror fails, it points
# Ubuntu's sources at archive.ubuntu.com, and security.ubuntu.com for
# -security, and tries once more; where that fails too, it fails. Given no
# package, the lists must update without an error.
set -euo pipefail

options=(
  -o Acquire::Retries=3
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
)
if (($# == 0)); then
  options+=(-o APT::Update::Error-Mode=any)
fi

attempt() {
  timeout 600 sudo apt-get "${options[@]}" update || return 1
  if (($# > 0)); then
    timeout 600 sudo apt-get "${options[@]}" install --yes "$@" || return 1
  fi
}

if attempt "$@"; then
  exit 0
fi

echo "::warning::the runner's apt mirror failed; trying archive.ubuntu.com and security.ubuntu.com"
# shellcheck source=/dev/null
. /etc/os-release
sudo tee /etc/apt/sources.list.d/ubuntu.sources >/dev/null <<EOF
Types: deb
URIs: http://archive.ubuntu.com/ubuntu/
Suites: ${VERSION_CODENAME} ${VERSION_CODENAME}-updates ${VERSION_CODENAME}-backports
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg

Types: deb
URIs: http://security.ubuntu.com/ubuntu/
Suites: ${VERSION_CODENAME}-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
if [[ -f /etc/apt/sources.list ]]; then
  sudo sed -i -E '/^deb .*(ubuntu\.com|mirror\+file)/s/^/# /' /etc/apt/sources.list
fi

if attempt "$@"; then
  exit 0
fi
echo "::error::apt failed from the runner's mirror and from archive.ubuntu.com and security.ubuntu.com"
exit 1
