# ADR-0018: A kind is data, and an event carries its kind's attributes

**Status:** Accepted
**Date:** 2026-11-10

## Context

ADR-0003 gives every event the same fields, among them `version` and
`prerelease`, and four kinds: `release`, `package-version`, `feed-entry` and
`message`. ADR-0015 makes every event an app sends a `message`.

An app announces more than messages: a deploy, an incident, a build. Each
has facts of its own, such as a region, an environment or a count, which no
fixed field holds. A `version` field on every event says nothing about a
feed entry, and a template that prints it for one renders `none`.

## Decision

1. **An event carries its kind's attributes, by name.** `version` and
   `prerelease` are attributes of the versioned kinds, not fields of every
   event. A template reads them as `event.attributes.version` and
   `event.attributes.prerelease`, and no other name reads them.
2. **A kind is data.** A kind is a name in lowercase letters and digits,
   words joined by `-`, and what it describes:

   | Part | Holds |
   |------|-------|
   | `description` | What it is, in a sentence |
   | `versioned` | Whether its events carry `version` (text, required) and `prerelease` (a flag) |
   | `links` | The names of the links that become its buttons, in order |
   | `attributes` | Each attribute by name: its `type` (`text`, `flag`, `number` or `url`), and whether it is `required` |

   herald ships `release` and `package-version`, both versioned, with
   `release` and `changelog` links; `feed-entry`, with an `entry` link; and
   `message`, with `release` and `changelog` links.
3. **The config declares more kinds under `[kinds.<name>]`.** A declared
   kind may not take a name herald ships, and gets `version` and
   `prerelease` only through `versioned = true`. An attribute's or a link's
   name is lowercase letters, digits and `_`, starting with a letter, so a
   template can read it.
4. **An app sends any kind the config knows.** Its event names its `kind`,
   `message` where it names none, and its `attributes`. herald refuses an
   event whose kind it does not know, or whose attributes are missing,
   unknown or of another type, and names the attribute.
5. **A route's `kinds` take any kind the config knows**, and an unknown
   name is refused at load. A route with `versions` takes only versioned
   kinds.
6. **The config is checked against each kind.** Each route's templates are
   dry-rendered against a sample of every kind it can take: its `kinds`, or
   its sources' kinds, which for an app are every kind. A template that
   prints an attribute a kind lacks is named with the attribute's path:
   "it prints event.attributes.version, which a feed-entry event does not
   carry". `{% if event.attributes.<name> %}` tests for one.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **Keep `event.version` and `event.prerelease` as aliases** | Two names for one value, and a template that reads the short name of an attribute a kind lacks renders `none` unchecked. |
| **Free-form attributes, undeclared** | Nothing to check a template or an event against at load, so a mistyped attribute reaches a channel. |
| **Kinds only in code** | An app could announce only messages, and a deploy's facts would go in the body as text no route or template could read. |

## Consequences

**Every template that printed `event.version` or tested `event.prerelease`
is rewritten** to `event.attributes.version` and `event.attributes.prerelease`.
`herald check` names each one it finds on a kind that lacks the attribute.

**A route without `kinds` that names an app is checked against every kind**,
so its templates render for each, or the route names the kinds it takes.

## Related

- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the event's fields, superseded in part here
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): the config, its schema and the dry render
- [ADR-0015](0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md): an app's event, superseded in part here
