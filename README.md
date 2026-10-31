# herald

herald announces what happened to the places people gather. It announces:

- a release on GitHub, GitLab, Forgejo or Gitea;
- a new version on Packagist, crates.io, npm or Docker Hub;
- an entry in an RSS or Atom feed;
- anything an app says.

It posts to Discord, Matrix, Slack and the Fediverse. Each post is formatted
for its destination and threaded where the destination can thread, and it is
never posted twice.

One `herald.toml` describes it all, and the same file runs as a GitHub
Action, a serverless endpoint or a container. Nothing long-lived is needed:
a repository with a workflow and one secret is enough to start.

> **Not released yet.** No version is tagged, and herald is published to no
> registry. The first release is 0.1.0, tagged when every source,
> destination, store and layer below is built.

## What it does

- **Sources:**
  - forge releases from GitHub, GitLab and Forgejo or Gitea;
  - package versions from Packagist, crates.io, npm and Docker Hub;
  - RSS and Atom feeds;
  - any app, through the CLI or the HTTP API.
- **Destinations:**
  - **Discord:** announcement, forum and text channels, with crossposting, a
    thread per announcement and opt-in role pings;
  - **Matrix**;
  - **Slack**;
  - **the Fediverse:** a Mastodon account, or herald's own ActivityPub actor.
- **Never twice:** each announcement has a deterministic key, and a run that
  was interrupted finishes what it started without repeating it.
- **Three layers:**
  - **Actions-only** needs nothing but GitHub.
  - **A serverless endpoint** adds interactions, webhooks and an HTTP API. It
    runs on Cloudflare Workers, AWS Lambda, Vercel, Netlify or Deno Deploy.
  - **A container** serves the same endpoint. `herald doctor` names anything
    your config asks for that the layer it runs in cannot serve.

## How it is designed

The design is recorded as decisions in [`.docs/decisions`](.docs/decisions/README.md).
Start with [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md),
which describes a sans-IO core behind ports. The same core runs in a GitHub
runner, a Worker and a container.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) says what a contribution needs. Report a
vulnerability privately, as [SECURITY.md](SECURITY.md) says. Everyone here
keeps the [code of conduct](CODE_OF_CONDUCT.md).

## Licence

herald is licensed under either of

- the [Apache License, Version 2.0](LICENSE-APACHE), or
- the [MIT licence](LICENSE-MIT),

at your option. Copyright NightWorksIO.

Unless you explicitly state otherwise, any contribution you intentionally
submit for inclusion in herald, as defined in the Apache-2.0 licence, is
dual-licensed as above, without any additional terms or conditions.
