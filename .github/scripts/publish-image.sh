#!/usr/bin/env bash
# Pushes one of herald's GHCR artefacts for the release TAG and signs it with
# cosign: `server`, the multi-arch image; `component`, the wasi:http component
# and its Spin build; `chart`, the Helm chart. Each digest, and the server's
# SBOM, is written to GITHUB_OUTPUT. A prerelease takes its own version's tag
# alone; a release also moves its minor and major tags.
set -euo pipefail

: "${REGISTRY:?REGISTRY names the registry and owner, such as ghcr.io/nightworksio}"
: "${TAG:?TAG names the release tag, such as v0.1.0}"
version="${TAG#v}"
tags=("${version}")
if [[ "${PRERELEASE:-false}" != "true" ]]; then
  tags+=("${version%.*}" "${version%%.*}")
fi

digest_of() {
  local reference="$1"
  docker buildx imagetools inspect --format '{{json .Manifest}}' "${reference}" | jq -r .digest
}

artefact="${1:?usage: publish-image.sh <server|component|chart>}"
case "${artefact}" in
  server)
    : "${COMMIT:?COMMIT is the verified commit}"
    named=()
    for tag in "${tags[@]}"; do named+=(--tag "${REGISTRY}/herald:${tag}"); done
    docker buildx create --use --name herald-release >/dev/null
    docker buildx build --platform linux/amd64,linux/arm64 --file Containerfile \
      --label "org.opencontainers.image.version=${version}" \
      --label "org.opencontainers.image.revision=${COMMIT}" \
      --provenance=false --sbom=false \
      --metadata-file "${RUNNER_TEMP}/server.json" --push "${named[@]}" .
    digest="$(jq -r '."containerimage.digest"' "${RUNNER_TEMP}/server.json")"
    sbom="${RUNNER_TEMP}/herald.spdx.json"
    syft scan "registry:${REGISTRY}/herald@${digest}" --output "spdx-json=${sbom}"
    cosign sign --yes "${REGISTRY}/herald@${digest}"
    printf 'digest=%s\nsbom=%s\n' "${digest}" "${sbom}" >> "${GITHUB_OUTPUT}"
    ;;
  component)
    for variant in component component-spin; do
      features=()
      [[ "${variant}" == "component-spin" ]] && features=(--features spin)
      cargo build --locked --release --target wasm32-wasip2 --package herald-wasi "${features[@]}"
      for tag in "${tags[@]}"; do
        wkg oci push "${REGISTRY}/herald-${variant}:${tag}" target/wasm32-wasip2/release/herald_wasi.wasm
      done
      digest="$(digest_of "${REGISTRY}/herald-${variant}:${version}")"
      cosign sign --yes "${REGISTRY}/herald-${variant}@${digest}"
      printf '%s=%s\n' "${variant//-/_}" "${digest}" >> "${GITHUB_OUTPUT}"
    done
    ;;
  chart)
    helm package deploy/helm/herald --version "${version}" --app-version "${version}" --destination "${RUNNER_TEMP}"
    helm push "${RUNNER_TEMP}/herald-${version}.tgz" "oci://${REGISTRY}/charts"
    digest="$(digest_of "${REGISTRY}/charts/herald:${version}")"
    cosign sign --yes "${REGISTRY}/charts/herald@${digest}"
    printf 'digest=%s\n' "${digest}" >> "${GITHUB_OUTPUT}"
    ;;
  *)
    echo "publish-image.sh knows server, component and chart" >&2
    exit 2
    ;;
esac
