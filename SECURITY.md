# Security policy

## Reporting a vulnerability

Report it privately, through GitHub:
[Report a vulnerability](https://github.com/nightworksio/herald/security/advisories/new)
on this repository's *Security* tab. Where you cannot use GitHub, email
<info@nightworks.io>.

Do not open a public issue or pull request for a vulnerability.

## What happens next

- Your report is acknowledged within three working days.
- A fix is published as a GitHub security advisory, with a CVE.

## Supported versions

The latest minor release of the current major version gets security fixes.
Once a new major version is released, the previous major's last minor gets
security fixes for six months.

No version is tagged yet; the first release is 0.1.0. Until it is tagged, a
fix lands on `main`, and a project that pins herald by commit takes it by
moving its pin.

## Verifying a release

Every release asset, the archives, the two WebAssembly components and
`SHA256SUMS`, carries a provenance attestation from herald's release workflow.
Check one with the GitHub CLI:

```sh
gh attestation verify herald-x86_64-unknown-linux-musl.tar.gz --repo nightworksio/herald \
  --signer-workflow nightworksio/herald/.github/workflows/release.yml
```

The image, the components on GHCR and the Helm chart are signed keylessly
with cosign, and attested too. With `<version>` such as `0.1.0`:

```sh
cosign verify ghcr.io/nightworksio/herald:<version> \
  --certificate-identity-regexp '^https://github.com/nightworksio/herald/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
gh attestation verify oci://ghcr.io/nightworksio/herald:<version> --repo nightworksio/herald
```

`herald-component`, `herald-component-spin` and `charts/herald` verify the
same way. herald's Action verifies the archive it downloads before it runs it.

## Scope

In scope is what this repository ships:

- the `herald` CLI and every crate in this workspace;
- the GitHub Action, `action.yml`;
- the serverless drivers, the container image and the templates in `deploy/`;
- herald's handling of the secrets it is given: a token or key that reaches a
  log, an error, a dry run, a template or another host than its
  destination's;
- herald's verification of inbound requests: a forged webhook, interaction,
  application-service transaction or ActivityPub activity that herald
  accepts;
- the never-twice guarantee, where a crafted input makes herald post as a
  project somewhere its config does not route to.

A vulnerability in Discord, Matrix, Slack, a Fediverse server or a dependency
belongs to that project. Where herald's use of it is what makes it
exploitable, report it here.
