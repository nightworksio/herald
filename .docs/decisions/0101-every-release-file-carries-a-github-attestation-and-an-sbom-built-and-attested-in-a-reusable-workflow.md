# ADR-0101: Every release file carries a GitHub attestation and an SBOM, built and attested in a reusable workflow

**Status:** Accepted
**Date:** 2026-11-27

## Context

The release workflow builds six targets, signs the server image, the
`wasi:http` component, its Spin build and the Helm chart with cosign,
gives them build provenance attestations, gives the server image an SPDX
SBOM, and attests the archives and `SHA256SUMS` (ADR-0009, ADR-0012). The
binaries and components have no SBOM; no attestation bundle is a release
asset; build and attestation run in the release workflow itself.
GitHub's attestations alone are SLSA v1.0 Build Level 2; a reusable
workflow that builds and attests reaches Level 3, and verification then
names the reusable workflow as the signer with `--signer-workflow`
(<https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/increase-security-rating>).
`actions/attest` writes Sigstore bundles and outputs `bundle-path`
(<https://github.com/actions/attest>). Scorecard's Signed-Releases check
reads release asset names such as `*.sigstore.json`
(<https://github.com/ossf/scorecard/blob/main/docs/checks.md#signed-releases>).

## Decision

1. **One signing system for files: GitHub's attestations.** Each
   attestation's bundle is attached to the release as
   `<asset>.sigstore.json`, so a file can be checked offline and with
   cosign as well as with `gh attestation verify`. cosign keeps signing
   the OCI artifacts.
2. **An SBOM for everything shipped.** Each binary and component is built
   with `cargo auditable`; per target an SPDX JSON and a CycloneDX JSON,
   named `herald-<target>.spdx.json` and `herald-<target>.cdx.json`, are
   attached and attested against their archive. The image keeps its SPDX
   attestation and gains a CycloneDX one.
3. **`actions/attest`** replaces `actions/attest-build-provenance` and
   `actions/attest-sbom`.
4. **Build Level 3.** Build and attestation run in a reusable workflow the
   release workflow calls; the release steps that follow cannot reach the
   signing. Every verification command in SECURITY.md and
   `deploy/README.md` names that reusable workflow as the signer.
5. **Verification runs in the release.** The `publish` job runs each
   documented command, online and offline, against the release's own
   outputs before `gh release create`, so a broken verification path
   fails the release.
6. **This extends** ADR-0009's signed outputs and ADR-0007's signed supply
   chain.

## Alternatives

| Option | Why not |
|--------|---------|
| **A cosign signature on every file as well** | Two signing systems for one file, and two verification paths to keep working. |
| **Level 2 for the first release** | No release exists to keep compatible; moving the build later changes every verification identity after users rely on them. |

## Consequences

- Release immutability, a `v*` tag ruleset, the `release` environment's
  protection and making the packages public are repository and
  organization settings, set by the maintainer before or after the first
  release.
- A verifier names the reusable workflow as the signer.

## Related

- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): the supply chain
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): what herald ships
- [ADR-0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md): the release
