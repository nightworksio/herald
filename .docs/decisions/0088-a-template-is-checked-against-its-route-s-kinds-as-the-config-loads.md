# ADR-0088: A template is checked against its route's kinds as the config loads

**Status:** Accepted
**Date:** 2026-11-26

## Context

[ADR-0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md) has a template read a kind's attributes as
`event.attributes.<name>`, and its consequences name `herald check` as
the command that names each attribute a template prints that a kind its
route takes does not carry. herald has no `herald check`. The config
renders each template against a sample event of every kind its route takes
as it loads, and a template that prints an attribute the kind lacks keeps
the config from loading, named by its key.

## Decision

1. **The check runs as the config loads.** Each template is rendered
   against a sample event of each kind its route takes. One that prints an
   attribute the kind does not carry is refused with its key and the
   attribute, such as `templates.deploy.title: it does not render a sample
   release event: it prints event.attributes.region, which release events do
   not carry`.
2. **Every command that reads the config says so**, `herald doctor` among
   them, and none runs with a template it refused. There is no separate
   command for it.
3. **This supersedes in part** ADR-0018's consequence naming `herald check`.

## Alternatives

| Option | Why not |
|--------|---------|
| **A `herald check` command** | `herald doctor` already loads and judges the config; a second command would check the same config a second way, and a run that skipped it would still load a template that cannot render. |

## Consequences

**A template that cannot render for a kind its route takes never reaches a
run**: the config that holds it does not load.

## Related

- [ADR-0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md): a kind's attributes, superseded in part here
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): `herald doctor`
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): the config and its templates
