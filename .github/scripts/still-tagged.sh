#!/usr/bin/env bash
# Fails unless the checkout is COMMIT and the tag TAG still points at it
# (ADR-0012): every release job after verify builds, judges, publishes or
# announces that commit only, never the tag read again by name.
set -euo pipefail

: "${TAG:?TAG names the release tag}"
: "${COMMIT:?COMMIT is the commit of this run, which verify proved TAG names}"

checked_out="$(git rev-parse HEAD)"
if [ "${checked_out}" != "${COMMIT}" ]; then
  echo "::error::the checkout is ${checked_out}, not ${COMMIT}, the commit verify proved ${TAG} names."
  exit 1
fi
tagged="$(gh api "repos/${GITHUB_REPOSITORY}/commits/refs/tags/${TAG}" --jq .sha)"
if [ "${tagged}" != "${COMMIT}" ]; then
  echo "::error::${TAG} is at ${tagged} now, not at ${COMMIT}, the commit verify proved it named."
  exit 1
fi
echo "${TAG} is still at ${COMMIT}."
