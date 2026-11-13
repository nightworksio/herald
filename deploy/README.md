# Deploying herald

herald runs as one native binary, one wasm module, one `wasi:http` component
or one Fastly Compute service (ADR-0017). The native binary is the CLI,
`herald server` and `herald lambda`. The wasm module runs behind a thin shim
on each JavaScript host below.

`herald doctor --layer <host>` names what a config asks for that a host
cannot serve. It also says what asks the host for rounds: the cron the files
here ship, a poll every five minutes and housekeeping hourly (both every five
minutes on Lambda), and where the config's `[schedule]` disagrees with it.

| Host | Shim | Stores | Rounds |
|------|------|--------|--------|
| Cloudflare Workers | `worker/` | `d1`, `durable-object`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | cron triggers |
| Deno and Deno Deploy | `deno/` | `deno-kv`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | `Deno.cron` |
| Node and Bun | `node/` | `sqlite`, `postgres`, `deno-kv`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | the config's `[schedule]` |
| Vercel | `vercel/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | Vercel cron |
| Netlify | `netlify/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | scheduled functions |
| AWS Lambda | `lambda/` | `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | an EventBridge schedule |
| Azure Functions | `azure/` | `sqlite`, `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | timer triggers |
| `wasmtime serve`, wasmCloud and other `wasi:http` hosts | `component/` | `dynamodb`, `git-branch`, `markers`, `repo-variables` | `POST /herald/round/*` |
| Spin | `spin/` | `sqlite`, `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | `POST /herald/round/*` |
| Fastly Compute | `fastly/` | `fastly-kv`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | `POST /herald/round/*` |
| A container, Compose, Kubernetes and Helm | `compose/`, `kubernetes/`, `helm/herald/` | `sqlite`, `postgres`, `dynamodb`, `git-branch`, `markers`, `repo-variables` | the config's `[schedule]` |

On every host that keeps `sqlite`, a transaction waits up to the store's
`busy` (5s) for the file's lock. On every host that keeps `postgres`, making a
connection gives up after the store's `connect_timeout` (10s), and the database
cancels a statement after `statement_timeout` (30s). herald keeps one
connection, and makes it again when the database closes it. A transaction that
was already on that connection fails and is not run again.

## The container image

`ghcr.io/nightworksio/herald` runs `herald server` as user 65532 on a
distroless static base, for `linux/amd64` and `linux/arm64`. Each release tags
it with its version, and a release that is not a prerelease also with its minor
and major versions. It is signed with cosign keylessly and carries an
attestation of its provenance and of its SBOM:

```sh
cosign verify ghcr.io/nightworksio/herald:0 \
  --certificate-identity-regexp '^https://github.com/nightworksio/herald/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
gh attestation verify oci://ghcr.io/nightworksio/herald:0 --owner nightworksio
```

| Path or setting | What it holds |
|-----------------|---------------|
| `/etc/herald/herald.toml` | the config, as `HERALD_CONFIG` names it |
| `/run/secrets` | a file per secret, as `HERALD_SECRETS_DIR` names it |
| `/var/lib/herald` | the volume SQLite keeps its file in, named in the config as `path = "/var/lib/herald/herald.sqlite"` |
| `HERALD_LISTEN` | `0.0.0.0:8080`, where the server listens |

Its health check is `herald health`, which asks the server's `/healthz` and
exits 0 where it answers.

## Compose

`compose/compose.yaml` runs the image with SQLite on a volume, reading
`compose/herald.toml` and each secret from a file under `compose/secrets/`.
`compose/compose.postgres.yaml`, added with `-f`, keeps records in a Postgres
beside it, reading `compose/herald.postgres.toml`. Its
`secrets/HERALD_POSTGRES_URL` is
`postgres://herald:<password>@postgres/herald?sslmode=disable`, with the
password `secrets/POSTGRES_PASSWORD` holds. The port is published on
`127.0.0.1` alone, for a reverse proxy in front of it to reach.

```sh
docker run --rm ghcr.io/nightworksio/herald:0 init | head -n 1 | cut -d= -f2- > secrets/HERALD_MARKER_SECRET
docker compose up -d --wait
docker compose -f compose.yaml -f compose.postgres.yaml up -d --wait
```

## Kubernetes

`kubernetes/` is a kustomization: one replica with a volume claim for SQLite,
the config as a ConfigMap made from `kubernetes/herald.toml`, and secrets from
the Secret `herald-secrets`, mounted at `/run/secrets`.

```sh
kubectl create secret generic herald-secrets --from-file=HERALD_MARKER_SECRET=./marker
kubectl apply -k kubernetes/
```

## Helm

The chart in `helm/herald/` is pushed with each release to
`oci://ghcr.io/nightworksio/charts/herald`, signed and attested as the image
is. `config` holds `herald.toml` but for its `[server]` and `[schedule]`
tables, which `server.*` and `schedule.*` write, each key the config's own.
`secrets.values` makes the Secret, or `secrets.existingSecret` names one.

```sh
helm install herald oci://ghcr.io/nightworksio/charts/herald --version 0.1.0 \
  --set-file config=herald.toml --set server.jobs=16 --set schedule.poll=10m \
  --set secrets.existingSecret=herald-secrets
```

## Building the wasm module

`deploy/build.sh` builds `herald-wasm` for `wasm32-unknown-unknown` and
writes its bindings to `deploy/pkg`, which every JavaScript shim imports. It
needs `wasm-bindgen` at the version `Cargo.lock` pins.

## Secrets

Every shim reads the secrets `herald.toml` names from the host's own
environment or secret store: `wrangler secret put` on a Worker, the
project's environment variables on Deno Deploy, Vercel and Netlify, and the
process environment on Node and Bun.

A process reads a secret from its environment, from the file its
`<NAME>_FILE` variable names, or from the file named as it is in the directory
`HERALD_SECRETS_DIR` names. Each file is read when the secret is asked for,
less one line ending. A secret given more than one way is refused, naming the
clash.

## Requests herald makes on others' behalf

herald fetches ActivityPub actors' keys and delivers to their inboxes only on
public HTTPS origins. The Node, Bun, Deno, Vercel and Netlify shims resolve
each such host themselves, and connect only where every address it resolves
to is public. A Worker's `fetch` reaches no private network. The component and
the Fastly service resolve nothing themselves, so they fetch nothing of this
until told their host's egress is guarded, as
[below](#fetching-on-others-behalf-where-herald-cannot-resolve-names) says.

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
deploy it to Deno Deploy. `HERALD_CONFIG` names another config file. It
listens on `HERALD_LISTEN`, or where unset on the config's `[server]
listen`; Deno Deploy chooses its own. `HERALD_POLL_CRON` and
`HERALD_HOUSEKEEP_CRON` set the rounds' crons, every five minutes and hourly
where unset.

## Node and Bun

Run `node deploy/node/server.mjs` or `bun deploy/node/server.mjs` beside
`herald.toml`. The server listens on `HERALD_LISTEN`, or where unset on
the config's `[server] listen` (`127.0.0.1:8080` by default), and runs
each round at the period the config's `[schedule]` names. `HERALD_CONFIG`
names another config file; files it names are read beside it.

- `sqlite` uses `node:sqlite` on Node 24 or newer, and `bun:sqlite` on Bun.
  The file is readable by its owner alone.
- `postgres` needs the `pg` package installed beside the shim.
- `deno-kv` needs the `@deno/kv` package installed beside the shim.
- `HERALD_TIMEOUT_MS` bounds each request herald sends in place of the
  config's `[http] total`.

## Vercel

Deploy `vercel/` with `herald.toml` and `pkg/` included; add any template
file the config names to `includeFiles` in `vercel.json`, which herald reads
beside the config. `vercel.json` sends every path to the function and
schedules both rounds at `/herald/round/*`.
Vercel calls them with the project's `CRON_SECRET`, which herald takes as
`HERALD_ROUND_SECRET` where that is unset; a round asked for without it is
refused.

## Netlify

Deploy `netlify/` with `herald.toml` and `pkg/` included; add any template
file the config names to `included_files` in `netlify.toml`, which herald
reads beside the config. The `poll` and `housekeep` scheduled functions run
the rounds.

## AWS Lambda

Put the `herald` binary for `aarch64-unknown-linux-gnu` and `herald.toml`
beside `lambda/bootstrap`, and deploy `lambda/template.yaml` with SAM.

- The function answers its URL at once and hands the work a request leaves
  to itself as an event invocation.
- The `Rounds` schedule polls, keeps house and forgets the keys read longest
  ago.
- `Table` names the DynamoDB table the function may read and write.
- `Memory`, `Timeout` and `LogRetention` size the function and its logs: 256
  MB, 900 seconds and 30 days where not given.
- `Concurrency` reserves how many invocations may run at once; none is
  reserved where it is empty. An announcement's lease outlasts the longest
  invocation, so two invocations never send one announcement's steps
  twice.

## Asking for a round over HTTP

Every host runs a round when asked over HTTP: `POST /herald/round/poll` or
`/herald/round/housekeep`, carrying `Authorization: Bearer` and the secret
`HERALD_ROUND_SECRET`. With that secret unset, no request runs a round. The
`wasi:http` component, Spin and Fastly Compute have no timer of their own, so
call them this way from any scheduler.

## `wasi:http` hosts

Each release pushes the component as an OCI artifact,
`ghcr.io/nightworksio/herald-component`, and its Spin build as
`ghcr.io/nightworksio/herald-component-spin`, each signed and attested as the
image is. Fetch one with `wkg oci pull`, or build it with `cargo build --release --target wasm32-wasip2 -p herald-wasi`, which
needs a clang that targets wasm. Run `wasmtime serve -Scli --dir=. herald_wasi.wasm`
beside `herald.toml`, passing secrets with `--env`. `HERALD_CONFIG` names another
config file, and `HERALD_TIMEOUT_MS` bounds each request herald sends in place
of the config's `[http] total`.

## Fetching on others' behalf where herald cannot resolve names

ActivityPub fetches and delivers to whatever origin an actor names. The
`wasi:http` component, Spin and Fastly Compute resolve names out of herald's
sight, so herald can refuse a private address written literally but not a name
that resolves to one. These hosts refuse every such fetch, and ActivityPub with
it, until `HERALD_EGRESS_GUARDED` is `true`. Set it, in the environment or
Fastly's Config Store, only once the host's own egress refuses private,
loopback and link-local addresses. Routes to configured origins are unaffected.

## Spin

Build with `--features spin` as well, and run `spin up` from `spin/`. `sqlite`
keeps records in Spin's database `default`, or the one `HERALD_SPIN_DATABASE`
names, and `postgres` connects to the store's URL. A component built with this
feature runs on Spin alone.

`allowed_outbound_hosts` in `spin.toml` admits any HTTPS origin, as the origins
a config names are not known here. List yours in its place where nothing fetches
on others' behalf. For ActivityPub, keep it and have Spin refuse private addresses,
which it checks after resolving a name:

```sh
spin up --runtime-config-file runtime-config.toml --env HERALD_EGRESS_GUARDED=true
```

## Fastly Compute

Build with `fastly compute build` from `fastly/`, which needs a clang that
targets wasm, and enable dynamic backends on the service.

- The Config Store `herald` holds `herald.toml`, each template file by its name,
  and `GITHUB_REPOSITORY`. An item holds at most 8 KB.
- The Secret Store `herald` holds every secret `herald.toml` names.
- `[store] kind = "fastly-kv"` needs the service linked to the KV Store it names.
  A delete first claims the item at its generation, as Fastly's delete takes
  none.

## Azure Functions

Put the `herald` binary for `x86_64-unknown-linux-gnu` and `herald.toml`
beside `azure/host.json`, and publish `azure/` as a custom-handler function
app. `herald server` listens on Azure's port and answers every path through
the `herald` function.

- Outside HTTP is served under `host.json`'s route prefix, `api`, so a
  webhook's URL is `https://<app>/api/hooks/<source>`. herald reads the prefix
  from `host.json`, or from the app setting
  `AzureFunctionsJobHost__extensions__http__routePrefix` where it is set.
- The `poll` and `housekeep` timers run a round each time they fire. No
  request from outside reaches their paths, which lie outside the prefix.
- With an empty prefix the timers' paths would be reachable, so herald runs
  no round there. Delete the `poll` and `housekeep` functions and ask for
  rounds as every host can, below.
