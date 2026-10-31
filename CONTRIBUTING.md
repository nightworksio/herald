# Contributing

This page says what a contribution needs, how to check one before you push
it, and where the design is written down. A vulnerability is reported
privately, as [SECURITY.md](SECURITY.md) says, and everyone here keeps the
[code of conduct](CODE_OF_CONDUCT.md).

## Setup

- **Rust through rustup.** `rust-toolchain.toml` pins the toolchain, with
  rustfmt, clippy, `llvm-tools-preview` and the `wasm32-unknown-unknown`
  target, and rustup installs it on the first `cargo` command.
- **cargo-nextest** runs the suite.

## The local gates

CI runs every gate on every pull request. Before you push, run the ones for
what you changed:

| You changed | Run |
|-------------|-----|
| anything | `typos` and `.github/scripts/line-cap.sh` |
| Rust | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and the tests of what you changed: `cargo nextest run -p <crate>` |
| `Cargo.toml` or `Cargo.lock` | `cargo deny check` |
| Markdown | `markdownlint-cli2 "**/*.md"` and `lychee --config lychee.toml .` |
| a workflow | `actionlint` and `zizmor .` |

Leave these to CI, which runs them on every pull request:

- **coverage**, `cargo llvm-cov nextest`, which fails under 100% of lines;
- **mutation testing**, cargo-mutants in shards, which fails on any missed
  mutant (ADR-0008);
- **msrv**, the suite on the oldest supported toolchain;
- **wasm**, the core built for `wasm32-unknown-unknown`;
- **audit**, CodeQL and gitleaks.

The tools CI installs are pinned by version and SHA-256 in
[`.github/scripts/install-tool.sh`](.github/scripts/install-tool.sh).

Each tool reads its configuration from the repository's root: `typos.toml`,
`.markdownlint.jsonc` with `.markdownlint-cli2.jsonc`, and `lychee.toml`.

## Commits and pull requests

- **Conventional subjects.** Every commit subject, and the pull request's
  title, is `type(scope): subject`. The type is one of `feat`, `fix`, `docs`,
  `refactor`, `test`, `chore`, `ci`, `perf`, `build`, `style` and `revert`.
  The scope is optional and lowercase, and a `!` before the colon marks a
  breaking change.
- **The title and body become the commit on `main`.** `main` takes only
  squash merges, whose subject is the pull request's title and whose message
  is its body.
- **The body ends with your sign-off.** Its last line is
  `Signed-off-by: Your Name <you@example.com>`, certifying the
  [Developer Certificate of Origin](https://developercertificate.org/) for
  the change.
- **A change that follows a decision names it** in the body, by its ADR
  number.
- **A breaking change says how to move.** A title with `!` needs a
  `## Migration` section in the body, saying what a user changes.

CI on a pull request from someone who is neither a member of the organisation
nor a collaborator on this repository starts once a maintainer approves the
run.

## Finding your way

- **[The decisions](.docs/decisions/README.md) are the design.** Each ADR
  gives a decision, the alternatives and why they lost.
  [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md) lays out
  the crates and the order in which they may name each other.
- **`crates/herald-arch` holds that order.** Its tests read `cargo metadata`
  and refuse an edge to a later layer or a sibling, an outside crate a layer
  does not allow, a runtime or I/O crate before the I/O layers, and a
  `clippy.toml` that would lift the workspace's bans on ambient I/O.

## Maintainers and decisions

- **There is one maintainer,** as [`.github/CODEOWNERS`](.github/CODEOWNERS)
  says.
- **Design is decided by ADR.** An accepted decision changes only through a
  new ADR that supersedes it, linked both ways, as
  [the decisions' rules](.docs/decisions/README.md) say.
- **A proposal becomes an ADR by pull request.** It adds a decision record
  under the next unused number, written as the others are, with its row in
  the index. The maintainer accepts it by merging it.
