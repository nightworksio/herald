# Presets: tables herald writes for a config

A `[presets.<name>]` table adopts a preset: tables herald writes for it as
the config loads, each named `<name>-` and then as the preset names it
([ADR-0092](decisions/0092-a-preset-is-shipped-or-local-toml-a-table-adopts-by-name-and-overrides-key-by-key.md)).
A key the table gives wins over the preset's, and one the preset does not
take is refused:

```toml
[presets.releases]
use = "herald:announce-releases"
destination = "discord"
sources = ["herald"]
channel = "1566184281406177531"
prereleases = "exclude"
```

The [settings reference](settings.md#presets) lists each preset herald
ships, its parameters and the tables it writes. `herald preset show <name>`
prints the tables a preset writes, and `herald preset eject <name>` writes
them into `herald.toml` in place of the `[presets.<name>]` table.

## Local presets

A local preset is a file inside `herald.toml`'s directory,
`use = "./presets/team.toml"`, reached through no link that leads outside
it. It declares `version = 1`, writes `routes`, `templates`, `views`,
`reports` and `rules` only, names no secret, and writes only the keys that
pick and shape what herald announces: a route's `id`, `sources`, `kinds`,
`prereleases`, `versions`, `match`, `area_paths`, `template`, `templates`
and `group`, a template's `title` and `body`, and of views, reports and
rules what they read and when. Every other key is refused with why: where
a route posts, whom it calls on, what it does to its posts, what herald
trusts of its place, a view's overviews and models and a rule's re-runs
come from the adopting table. Only herald's native binary reads one.

A mistake in a preset's tables is told at the line of the table that
adopts it, and a local preset's with its own file, line and column.
