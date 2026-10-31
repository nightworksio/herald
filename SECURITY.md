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
