// The component on wasmtime and on Spin, and the service on Viceroy, each
// answering real requests: herald's health, a round without and with its secret.
// An activity whose key lies on a name resolving to loopback shows what each
// reaches on others' behalf. A key id names no port, so the key's listener binds
// 443, which needs net.ipv4.ip_unprivileged_port_start at 443 or below.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { connect as connectTo, createServer } from "node:net";
import { join, relative } from "node:path";
import { after, before, test } from "node:test";

const SECRET = "local-round-secret";
const EGRESS = "deploy/tests/egress";
const SPIN = "deploy/spin";
const MANIFEST = "spin.toml";
const COMPONENT_SOURCE = /^source = "([^"]+)"$/m;
const KEY_PORT = 443;
const COMPONENT = "../../component/herald_wasi.wasm";
const RUNTIME_CONFIG = "../../spin/runtime-config.toml";
const PEM_LABEL = ["PRIVATE", "KEY"].join(" ");
const KEY = [
  `-----BEGIN ${PEM_LABEL}-----`,
  Buffer.from(
    readFileSync("crates/herald-crypto/fixtures/rsa-2048-pkcs8.test.hex", "utf8").trim(),
    "hex",
  ).toString("base64"),
  `-----END ${PEM_LABEL}-----`,
].join("\n");
const ENV = {
  HERALD_ACTIVITYPUB_KEY: KEY,
  GITHUB_TOKEN: "local-token",
  GITHUB_REPOSITORY: "nightworksio/herald",
  HERALD_STORE_KEY: "local-store-key",
};
const started = [];

before(() => {
  const shipped = readFileSync(join(SPIN, MANIFEST), "utf8");
  const [, source] = shipped.match(COMPONENT_SOURCE) ?? [];
  assert.ok(source, `${SPIN}/${MANIFEST} names no component source`);
  const moved = relative(EGRESS, join(SPIN, source));
  writeFileSync(join(EGRESS, MANIFEST), shipped.replace(COMPONENT_SOURCE, `source = ${JSON.stringify(moved)}`));
});

after(() => started.forEach((child) => child.kill()));

async function up(base) {
  for (let tries = 0; tries < 120; tries += 1) {
    try {
      const answer = await fetch(`${base}/healthz`);
      if (answer.status) return;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
  }
  throw new Error(`${base} never answered`);
}

async function answers(base) {
  const health = await fetch(`${base}/healthz`);
  assert.equal(health.status, 200);
  assert.equal(await health.text(), "herald is up");
  const unheld = await fetch(`${base}/herald/round/poll`, { method: "POST" });
  assert.equal(unheld.status, 401);
  for (const round of ["poll", "housekeep"]) {
    const held = await fetch(`${base}/herald/round/${round}`, {
      method: "POST",
      headers: { authorization: `Bearer ${SECRET}` },
    });
    assert.equal(held.status, 200, round);
    assert.equal(await held.text(), "the round ran");
  }
}

function raw(port, text) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    const socket = connectTo(port, "127.0.0.1", () => socket.write(text));
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("end", () => resolve(Buffer.concat(chunks).toString("latin1")));
    socket.on("error", reject);
  });
}

function run(command, args, cwd) {
  const child = spawn(command, args, { cwd, stdio: "inherit" });
  started.push(child);
}

function spinEnv() {
  return Object.entries(ENV).flatMap(([name, value]) => ["--env", `${name}=${value}`]);
}

function posted(port) {
  const body = "{}";
  const digest = createHash("sha256").update(body).digest("base64");
  return new Promise((resolve, reject) => {
    const asked = request(
      {
        host: "127.0.0.1",
        port,
        path: "/inbox",
        method: "POST",
        headers: {
          host: "herald.example",
          date: new Date().toUTCString(),
          digest: `SHA-256=${digest}`,
          signature: `keyId="https://localhost/actor",headers="(request-target) host date digest",signature="AAAA"`,
        },
      },
      (answer) => {
        answer.resume();
        answer.on("end", () => resolve(answer.statusCode));
      },
    );
    asked.on("error", reject);
    asked.end(body);
  });
}

async function reachedLoopback(port) {
  let connections = 0;
  const listener = createServer((socket) => {
    connections += 1;
    socket.destroy();
  });
  await new Promise((resolve) => listener.listen(KEY_PORT, resolve));
  const status = await posted(port);
  listener.close();
  assert.equal(status, 401);
  return connections > 0;
}

test("the component refuses a name on others' behalf where nothing guards its host", async () => {
  run(
    "wasmtime",
    [
      "serve",
      "-Scli",
      "--addr=127.0.0.1:8104",
      "--dir=.",
      ...Object.entries(ENV).map(([name, value]) => `--env=${name}=${value}`),
      COMPONENT,
    ],
    EGRESS,
  );
  await up("http://127.0.0.1:8104");
  assert.equal(await reachedLoopback(8104), false);
});

test("Spin told it is guarded reaches a name resolving to loopback without its block", async () => {
  run(
    "spin",
    [
      "up",
      "--listen=127.0.0.1:8105",
      ...spinEnv(),
      "--env",
      "HERALD_EGRESS_GUARDED=true",
    ],
    EGRESS,
  );
  await up("http://127.0.0.1:8105");
  assert.equal(await reachedLoopback(8105), true);
});

test("Spin's shipped block refuses a name resolving to loopback", async () => {
  run(
    "spin",
    [
      "up",
      "--listen=127.0.0.1:8106",
      "--runtime-config-file",
      RUNTIME_CONFIG,
      ...spinEnv(),
      "--env",
      "HERALD_EGRESS_GUARDED=true",
    ],
    EGRESS,
  );
  await up("http://127.0.0.1:8106");
  assert.equal(await reachedLoopback(8106), false);
});

test("the component answers on wasmtime", async () => {
  run(
    "wasmtime",
    [
      "serve",
      "-Scli",
      "--addr=127.0.0.1:8101",
      "--dir=.",
      `--env=HERALD_ROUND_SECRET=${SECRET}`,
      "herald_wasi.wasm",
    ],
    "deploy/component",
  );
  await up("http://127.0.0.1:8101");
  await answers("http://127.0.0.1:8101");
});

test("the component says it is up from its config alone, before it opens a store", async () => {
  run(
    "wasmtime",
    ["serve", "-Scli", "--addr=127.0.0.1:8108", "--dir=.", `--env=HERALD_ROUND_SECRET=${SECRET}`, `../../component/herald_wasi.wasm`],
    "deploy/tests/uncomposed",
  );
  await up("http://127.0.0.1:8108");
  const health = await fetch("http://127.0.0.1:8108/healthz");
  assert.equal(health.status, 200);
  assert.equal(await health.text(), "herald is up");
  const round = await fetch("http://127.0.0.1:8108/herald/round/poll", {
    method: "POST",
    headers: { authorization: `Bearer ${SECRET}` },
  });
  assert.equal(round.status, 500);
  const head = await raw(8108, "HEAD /herald/round/poll HTTP/1.1\r\nHost: herald.example\r\nConnection: close\r\n\r\n");
  const [lines, rest] = head.split("\r\n\r\n");
  assert.match(lines, /^HTTP\/1\.1 500/);
  assert.match(lines, /content-length: [1-9]/i);
  assert.equal(rest, "");
});

test("the component keeps records in Spin's SQLite", async () => {
  run(
    "spin",
    ["up", "--listen=127.0.0.1:8102", "--env", `HERALD_ROUND_SECRET=${SECRET}`],
    "deploy/spin",
  );
  await up("http://127.0.0.1:8102");
  await answers("http://127.0.0.1:8102");
});

test("Spin's own SQLite and Postgres keep every record a store keeps", async () => {
  const url = process.env.HERALD_TEST_POSTGRES;
  assert.ok(url, "HERALD_TEST_POSTGRES names the Postgres database the suite runs over");
  run("spin", ["up", "--listen=127.0.0.1:8107", "--env", `HERALD_TEST_POSTGRES=${url}`], "deploy/tests/spin-conformance");
  await up("http://127.0.0.1:8107");
  for (const database of ["sqlite", "postgres"]) {
    const answer = await fetch(`http://127.0.0.1:8107/conformance/${database}`);
    assert.equal(await answer.text(), "conforms", database);
    assert.equal(answer.status, 200, database);
  }
});

test("the service answers on Viceroy, keeping records in its KV Store", async () => {
  run("viceroy", [
    "serve",
    "-C",
    "deploy/fastly/fastly.toml",
    "--addr=127.0.0.1:8103",
    "target/wasm32-wasip1/release/herald-fastly.wasm",
  ]);
  await up("http://127.0.0.1:8103");
  await answers("http://127.0.0.1:8103");
});
