# Deploying herald

herald runs as one native binary or one wasm module (ADR-0017). The native
binary is the CLI, `herald server` and `herald lambda`. The wasm module runs
behind a thin shim on each JavaScript host below.

`herald doctor --layer <host>` names what a config asks for that a host
cannot serve.

| Host | Shim | Stores | Rounds |
|------|------|--------|--------|
| Cloudflare Workers | `worker/` | `d1`, `durable-object`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | cron triggers |
| Deno and Deno Deploy | `deno/` | `deno-kv`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | `Deno.cron` |
| Node and Bun | `node/` | `sqlite`, `postgres`, `deno-kv`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | the config's `[schedule]` |
| Vercel | `vercel/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | Vercel cron |
| Netlify | `netlify/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | scheduled functions |
| AWS Lambda | `lambda/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | an EventBridge schedule |
| Azure Functions | `azure/` | the stores `herald server` keeps records in | timer triggers |

## Building the wasm module

`deploy/build.sh` builds `herald-wasm` for `wasm32-unknown-unknown` and
writes its bindings to `deploy/pkg`, which every JavaScript shim imports. It
needs `wasm-bindgen` at the version `Cargo.lock` pins.

## Secrets

Every shim reads the secrets `herald.toml` names from the host's own
environment or secret store: `wrangler secret put` on a Worker, the
project's environment variables on Deno Deploy, Vercel and Netlify, and the
process environment on Node and Bun.

## Requests herald makes on others' behalf

herald fetches ActivityPub actors' keys and delivers to their inboxes only on
public HTTPS origins. The Node, Bun, Deno, Vercel and Netlify shims resolve
each such host themselves, and connect only where every address it resolves
to is public. A Worker's `fetch` reaches no private network.

## Cloudflare Workers

Put `herald.toml` beside `worker/wrangler.toml`, list any template files it
names in `worker/files.mjs`, and run `wrangler deploy` from `worker/`.

- `[store] kind = "d1"` reads the `HERALD_DB` binding: create the database
  with `wrangler d1 create herald` and set its id in `wrangler.toml`.
- `[store] kind = "durable-object"` keeps records in the `HeraldObject`
  Durable Object's SQLite storage, through the `HERALD` binding.
- A request's work runs after its answer, through `waitUntil`.
- The cron `HERALD_HOUSEKEEP_CRON` names keeps house; every other cron polls.

## Deno

Run `deno run --allow-read --allow-net --allow-env --allow-sys
--unstable-cron --unstable-kv deploy/deno/main.ts` beside `herald.toml`, or
deploy it to Deno Deploy. `HERALD_CONFIG` names another config file.
`HERALD_POLL_CRON` and `HERALD_HOUSEKEEP_CRON` set the rounds' crons, every
five minutes and hourly where unset.

## Node and Bun

Run `node deploy/node/server.mjs` or `bun deploy/node/server.mjs` beside
`herald.toml`. The server listens on `HERALD_LISTEN` (`127.0.0.1:8080`
where unset) and runs each round at the period the config's `[schedule]`
names.

- `sqlite` uses `node:sqlite` on Node 24 or newer, and `bun:sqlite` on Bun.
  The file is readable by its owner alone.
- `postgres` needs the `pg` package installed beside the shim.
- `deno-kv` needs the `@deno/kv` package installed beside the shim.
- `HERALD_TIMEOUT_MS` bounds each request herald sends, 10 seconds where
  unset.

## Vercel

Deploy `vercel/` with `herald.toml` and `pkg/` included. `vercel.json` sends
every path to the function and schedules both rounds, which Vercel calls with
the project's `CRON_SECRET`; a round asked for without it is refused.

## Netlify

Deploy `netlify/` with `herald.toml` and `pkg/` included. The `poll` and
`housekeep` scheduled functions run the rounds.

## AWS Lambda

Put the `herald` binary for `aarch64-unknown-linux-gnu` and `herald.toml`
beside `lambda/bootstrap`, and deploy `lambda/template.yaml` with SAM.

- The function answers its URL at once and hands the work a request leaves
  to itself as an event invocation.
- The `Rounds` schedule polls, keeps house and forgets the keys read longest
  ago.
- `Table` names the DynamoDB table the function may read and write.

## Azure Functions

Put the `herald` binary for `x86_64-unknown-linux-gnu` and `herald.toml`
beside `azure/host.json`, and publish `azure/` as a custom-handler function
app. `herald server` listens on Azure's port, answers every path through the
`herald` function, and runs a round each time the `poll` or `housekeep`
timer fires.
