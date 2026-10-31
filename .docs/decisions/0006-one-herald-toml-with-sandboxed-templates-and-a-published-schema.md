# ADR-0006: One `herald.toml`, with sandboxed templates and a published JSON Schema, validated before anything posts

**Status:** Accepted
**Date:** 2026-10-31

## Context

The same config runs as an Action, on a serverless platform and in a
container (ADR-0002), so it cannot live in any one of them. It cannot be
workflow inputs, a Worker's environment or a database table. It has to be a
file in the repository, reviewable in a pull request.

A config mistake found at post time is found in public: a broken template
renders in the channel, or a title over Discord's limit is refused after the
crosspost of the previous step. Everything that can be checked without
sending must be checked before anything is sent.

Templates are code, and the data they render is not herald's. A release body
is written by whoever can publish a release, and a feed entry by whoever runs
the feed. The template engine must not give either of them a way out of the
template.

## Decision

1. **One file, `herald.toml`.** The CLI reads `--config`, or `herald.toml` in
   the working directory. A serverless driver reads it bundled at deploy
   time, and the container from a mounted path. The first user's config is
   the documented example:

   ```toml
   #:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json
   version = 1

   [store]
   kind = "git-branch"

   [destinations.discord]
   kind = "discord"
   token = { secret = "HERALD_DISCORD_TOKEN" }

   [sources.php-mutation-gate]
   kind = "github"
   repository = "nightworksio/php-mutation-gate"

   [[routes]]
   id = "php-mutation-gate-releases"
   sources = ["php-mutation-gate"]
   kinds = ["release"]
   destination = "discord"
   channel = "1566184281406177531"
   template = "package-release"
   crosspost = true
   thread = { name = "{{ event.project }} {{ event.version }}", archive_after = "14d" }
   mentions = { roles = ["<the php-mutation-gate role's id>"] }

   [templates.package-release]
   title = "📦 {{ event.project }} {{ event.version }}{% if event.prerelease %} · pre-release{% endif %}"
   body = "{{ event.body }}"
   ```

   - `[store]` chooses one store and its options (ADR-0005).
   - `[destinations.*]` name the places herald posts to and their
     credentials.
   - `[sources.*]` name what herald listens to (ADR-0003).
   - `[[routes]]` connect them: an `id`, a filter over sources, kinds and
     pre-releases, a destination, a template, and the destination's own
     options.
   - `[templates.*]` hold the templates a route names.
   - `[apps.*]` hold the HTTP API's app tokens, by hash (ADR-0007).
   - `[schedule]` holds the cadence of polling and housekeeping where the
     driver keeps its own schedule.

2. **The config is typed, and each destination owns its route options.** The
   options a route may give depend on its destination's kind: `channel`,
   `crosspost` and `thread` for Discord, `room` for Matrix. Each destination
   crate defines its options type, and `herald-config` reads a route's
   options into the type its destination names (ADR-0001). An option the
   destination does not have is an error, not a silent no-op.

3. **Every error names the key and the fix.** An error gives the file, line
   and column, the key's path, what is wrong and what to write instead. For
   example: `herald.toml:17:15 routes[0].destination: no destination is named
   "announcements"; the destinations are "discord"`. Every error in the file
   is reported in one pass, not the first alone.

4. **A secret is a reference, never a value.** Where a credential is
   expected, the config takes `{ secret = "NAME" }`, which `Secrets` resolves
   at run time: the environment on a runner, the platform's secrets on a
   serverless platform, and a mounted file or the environment in a container.
   A plain string where a secret belongs is refused, with the fix in the
   message (ADR-0007).

5. **The JSON Schema is generated from the types and published.**
   - **The schemas.** `herald.schema.json`, for the config, and
     `herald-event.schema.json`, for an app's event (ADR-0003), are generated
     from the Rust types with `schemars`.
   - **Where they live.** Both are committed at the repository's root and
     attached to every release. CI fails when regenerating either changes it.
   - **Editor support.** The `#:schema` line gives an editor completion and
     validation as the file is written.

6. **Templates are minijinja, rendered in a sandbox.**
   - **Where a template comes from.** It is inline in the config, or in a file
     beside it (`file = "templates/release.md.j2"`) that the driver reads at
     load. minijinja itself has no filesystem: an `include` names another
     template in the config, and nothing else.
   - **Limits.** Rendering runs on a fuel limit, a recursion limit and a cap
     on output size. A template that loops forever runs out of fuel and
     fails at load, when it is dry-rendered.
   - **Undefined values.** Undefined is strict, so `{{ event.release_notes }}` is an
     error at load, not an empty string in the channel.
   - **The context.** The context holds `event`, `route` and `herald`, the
     running version. It holds no secret, no environment variable and no
     config value beyond the route's own id.
   - **Markdown out.** A template renders Markdown, which the render step
     converts to each destination's dialect (ADR-0004). Event text is escaped
     for that dialect, so a release body cannot write a mention or break out
     of a code block.

7. **The config is validated before anything posts.** Loading the config:
   - parses it and resolves every name: sources, destinations, templates and
     roles;
   - compiles every template;
   - dry-renders every template against sample events of every kind the route
     admits, a pre-release and a long body among them;
   - checks the parts with fixed limits against the destination's `Profile`:
     titles, thread names and Mastodon's character limit for a single post;
   - checks that every secret the run will need is named, and resolves each
     one before the first request.

   `herald preview` renders a route's templates against a sample event, or an
   event file, and prints them as each destination would show them.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **YAML** | Its implicit typing turns `on` into a boolean and `1.10` into a number, and both are plausible values here: a source named `on`, a version `1.10`. TOML has neither surprise, and Rust's tooling for it keeps spans for the errors. |
| **Config as workflow inputs** | It would exist only on Actions, and the same project on a Worker would describe its routes again. |
| **Tera or Handlebars** | Tera has no fuel limit. Handlebars is logic-less to the point that a pre-release marker becomes a helper written in Rust. minijinja is sandboxed by design, has fuel, and is the Jinja dialect adopters already know. |
| **Secrets inline, with a warning** | A token committed once is leaked forever, however loud the warning was. A reference is the only form the config accepts. |
| **A hand-written schema** | It would drift from the types that read the config. Generated and checked in CI, the two cannot disagree. |

## Consequences

**A config review is a pull-request review.** The file, its templates and
its routes are in the repository, and doctor validates them in CI
(ADR-0010).

**Template mistakes fail at load.** A typo in a field name, an endless loop
or a title over a limit is reported before the first request, never in a
channel.

**An editor knows the config.** Completion and validation come from the
published schema.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): `herald-config` and the render step
- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the event schema
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): dialects, profiles and route options
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets and app tokens
- [ADR-0010](0010-herald-announces-itself-with-itself.md): herald's own `herald.toml`
