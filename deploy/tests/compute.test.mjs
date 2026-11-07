// The component on wasmtime and on Spin, and the service on Viceroy, each
// answering real requests: herald's health, a round without and with its secret.
// An activity whose key lies on a name resolving to loopback shows what each
// reaches on others' behalf.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { request } from "node:http";
import { createServer } from "node:net";
import { after, test } from "node:test";

const SECRET = "local-round-secret";
const EGRESS = "deploy/tests/egress";
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
};
const started = [];

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

function run(command, args, cwd) {
  const child = spawn(command, args, { cwd, stdio: "inherit" });
  started.push(child);
}

function spinEnv() {
  return Object.entries(ENV).flatMap(([name, value]) => ["--env", `${name}=${value}`]);
}

function posted(port, keyOn) {
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
          signature: `keyId="https://localhost:${keyOn}/actor",headers="(request-target) host date digest",signature="AAAA"`,
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
  await new Promise((resolve) => listener.listen(0, resolve));
  const status = await posted(port, listener.address().port);
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

test("the component keeps records in Spin's SQLite", async () => {
  run(
    "spin",
    ["up", "--listen=127.0.0.1:8102", "--env", `HERALD_ROUND_SECRET=${SECRET}`],
    "deploy/spin",
  );
  await up("http://127.0.0.1:8102");
  await answers("http://127.0.0.1:8102");
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
