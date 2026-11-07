// The component on wasmtime and on Spin, and the service on Viceroy, each
// answering real requests: herald's health, a round without and with its secret.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { after, test } from "node:test";

const SECRET = "local-round-secret";
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
