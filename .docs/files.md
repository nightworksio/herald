# The files a config may name

A template's `file`, a source's `members.file`, an ActivityPub `lists[].file`
and `http.ca_file` are read only from beside `herald.toml`, as a path of names
with no `..` step, or from an absolute directory `[files] allowed` lists
([ADR-0092](decisions/0092-a-preset-is-shipped-or-local-toml-a-table-adopts-by-name-and-overrides-key-by-key.md)
decision 8). Every link of the file and of its directory is followed before
the check.

## What the host lets `[files] allowed` reach

`[files] allowed` in `herald.toml` may narrow what the host lets a config reach,
never widen it. The host sets that bound in `HERALD_FILES_ALLOWED`: absolute
directories joined as `PATH` joins its own (`:` on Linux and macOS, `;` on
Windows).

| The host | `[files] allowed` may list |
|----------|----------------------------|
| sets `HERALD_FILES_ALLOWED` | directories inside one it lists |
| does not set it | directories inside the config's own directory |
| reads files by name (Fastly's Config Store, a JavaScript object of files, the WASI component) | none |

Each directory `[files] allowed` lists, and each the host lists, is compared
with every link followed and step by step, so `/workspace` is not inside
`/work`, and a link inside a listed directory that leads out of it is outside.
Each file is judged again as it is read: its real path lies inside the
directory as the host resolved it at load and inside the host's bound, and
the native binary and the JavaScript shims read it through the handle they
opened once that handle is found to be the file at that real path. A link
re-pointed after the load, or swapped between the check and the open, reads
nothing. A directory outside the bound stops the load. The error names the key,
`files.allowed`, its line and why, and never the path written there. A
`HERALD_FILES_ALLOWED` that lists a relative path or one that is not a
directory stops the load, naming the variable and none of its value.

`herald doctor` says what the host lets `[files] allowed` reach and where it
took that from: the directories `HERALD_FILES_ALLOWED` lists, or the config's
own directory where it is not set.

The native binary, the image, the Action, `herald server` and Lambda read
`HERALD_FILES_ALLOWED` from their environment. The Node, Bun, Deno, Vercel and
Netlify shims pass it from theirs (`deploy/files.mjs`).

## Where a pull request cannot set it

The bound holds against a pull request only where the pull request cannot
write what sets `HERALD_FILES_ALLOWED`. A file in the checkout never sets it.

On GitHub Actions the variable is set in the `env` of the workflow, job or step
that runs herald, so it is as safe as that workflow file. "Each workflow run
will use the version of the workflow that is present in the associated commit
SHA or Git ref of the event"
([Workflows](https://docs.github.com/en/actions/concepts/workflows-and-actions/workflows)),
and that commit depends on the event
([Events that trigger workflows](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)):

| Event | `GITHUB_SHA` | Can the pull request change the bound? |
|-------|--------------|----------------------------------------|
| `pull_request` | "Last merge commit on the `GITHUB_REF` branch", the pull request's merge branch | yes: the workflow file of that merge commit runs |
| `pull_request_target` | "Last commit on default branch" | no: the default branch's workflow runs |
| `push` | "Tip commit pushed to the ref" | only by being pushed to that ref |
| `release` | "Last commit in the tagged release" | only by being in the tagged commit |
| `schedule`, `workflow_run` | "Last commit on default branch" | no |

So a workflow that runs herald on `pull_request` gives the pull request the
bound as well as `herald.toml`; run herald on an event whose workflow comes
from a branch the pull request cannot write. On any host, set
`HERALD_FILES_ALLOWED` where only the host's operator writes: the service's
environment, the container's, or the function's settings.
