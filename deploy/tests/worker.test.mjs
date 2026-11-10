import assert from "node:assert/strict";
import { test } from "node:test";
import { Miniflare } from "miniflare";

const DEPLOY = new URL("../", import.meta.url).pathname;
const COMPATIBLE = "2026-07-01";
const SECRET = "local-round-secret";
const HOUSEKEEP = "17 * * * *";
const POLL = "*/5 * * * *";
const D1 = 'version = 1\n[store]\nkind = "d1"\n[sources.news]\nkind = "feed"\nurl = "https://feed.invalid/feed.xml"\n';
const POLLED = /^news: stopped: the host could not be reached: .*\. The next poll asks again\.\n$/;

function shipped(config, { bindings = {}, durableObjects = { HERALD: { className: "HeraldObject", useSQLite: true } } } = {}) {
  const said = { out: "" };
  const worker = new Miniflare({
    modulesRoot: DEPLOY,
    modules: [
      { type: "ESModule", path: `${DEPLOY}worker/worker.mjs` },
      { type: "ESModule", path: `${DEPLOY}worker/files.mjs` },
      { type: "ESModule", path: `${DEPLOY}worker/sql.mjs` },
      { type: "Text", path: `${DEPLOY}worker/herald.toml`, contents: config },
      { type: "ESModule", path: `${DEPLOY}host.mjs` },
      { type: "ESModule", path: `${DEPLOY}pkg/herald_wasm.js` },
      { type: "CompiledWasm", path: `${DEPLOY}pkg/herald_wasm_bg.wasm` },
    ],
    d1Databases: ["HERALD_DB"],
    durableObjects,
    bindings: { HERALD_HOUSEKEEP_CRON: HOUSEKEEP, HERALD_ROUND_SECRET: SECRET, ...bindings },
    compatibilityDate: COMPATIBLE,
    cf: false,
    unsafeInspectDurableObjects: true,
    handleRuntimeStdio(stdout) {
      stdout.on("data", (chunk) => (said.out += chunk));
    },
  });
  return { worker, said };
}

async function within(worker, check) {
  try {
    await check();
  } finally {
    await worker.dispose();
  }
}

async function ran(worker, round) {
  return worker.dispatchFetch(`http://localhost/herald/round/${round}`, {
    method: "POST",
    headers: { authorization: `Bearer ${SECRET}` },
  });
}

async function heard(said, pattern) {
  for (let tries = 0; tries < 100 && !pattern.test(said.out); tries += 1) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  assert.match(said.out, pattern);
}

test("a Durable Object's own SQLite storage keeps every record a store keeps, and it and D1 pass the database suite", async () => {
  const worker = new Miniflare({
    modules: true,
    scriptPath: new URL("worker/conformance.mjs", import.meta.url).pathname,
    modulesRules: [
      { type: "ESModule", include: ["**/*.js", "**/*.mjs"] },
      { type: "CompiledWasm", include: ["**/*.wasm"] },
    ],
    durableObjects: { KEPT: { className: "Kept", useSQLite: true } },
    d1Databases: ["DB"],
    compatibilityDate: COMPATIBLE,
    cf: false,
  });
  await within(worker, async () => {
    const answered = await worker.dispatchFetch("http://localhost/");
    assert.equal(await answered.text(), "conforms");
    assert.equal(answered.status, 200);
    for (const database of ["sql", "d1"]) {
      const suite = await worker.dispatchFetch(`http://localhost/${database}`);
      assert.equal(await suite.text(), "conforms", database);
      assert.equal(suite.status, 200, database);
    }
    const refused = await worker.dispatchFetch("http://localhost/refusing");
    assert.equal(await refused.text(), "the store could not be reached: the database refused the statements: the database refused");
    assert.equal(refused.status, 500);
  });
});

test("the Worker answers, keeping its records in D1 through one batch per transaction", async () => {
  const { worker } = shipped(D1);
  await within(worker, async () => {
    const health = await worker.dispatchFetch("http://localhost/healthz");
    assert.equal(health.status, 200);
    assert.equal(await health.text(), "herald is up");
    const unheld = await worker.dispatchFetch("http://localhost/herald/round/housekeep", { method: "POST" });
    assert.equal(unheld.status, 401);
    const held = await ran(worker, "housekeep");
    assert.equal(held.status, 200);
    assert.equal(await held.text(), "the round ran");
    const db = await worker.getD1Database("HERALD_DB");
    assert.deepEqual(await db.prepare("SELECT layout FROM herald_meta").all().then((found) => found.results), [{ layout: 2 }]);
  });
});

test("the Worker keeps its records in its Durable Object's SQLite storage, reached through the object", async () => {
  const { worker } = shipped('version = 1\n[store]\nkind = "durable-object"\n');
  await within(worker, async () => {
    const held = await ran(worker, "housekeep");
    assert.equal(held.status, 200);
    assert.equal(await held.text(), "the round ran");
    const storage = await worker.unsafeGetDurableObjectStorage("", "HeraldObject", { name: "herald" });
    assert.deepEqual(await storage.exec("SELECT layout FROM herald_meta"), [{ layout: 2 }]);
  });
});

test("a cron runs the round it names, and each holds the sessions on the object herald keeps them on", async () => {
  const { worker, said } = shipped(D1);
  await within(worker, async () => {
    const scheduled = await worker.getWorker();
    const housekept = await scheduled.scheduled({ cron: HOUSEKEEP });
    assert.equal(housekept.outcome, "ok");
    const sessions = await worker.getDurableObjectNamespace("HERALD");
    assert.deepEqual(await worker.listDurableObjectIds("HERALD"), [sessions.idFromName("herald").toString()]);
    assert.equal(said.out, "");
    const polled = await scheduled.scheduled({ cron: POLL });
    assert.equal(polled.outcome, "ok");
    await heard(said, POLLED);
  });
});

test("the sessions are held on the binding and object the variables name, and without one the rounds run alone", async () => {
  const named = shipped(D1, {
    bindings: { HERALD_SESSIONS_BINDING: "SESSIONS", HERALD_SESSIONS_OBJECT: "held" },
    durableObjects: { SESSIONS: { className: "HeraldObject", useSQLite: true } },
  });
  await within(named.worker, async () => {
    const scheduled = await named.worker.getWorker();
    assert.equal((await scheduled.scheduled({ cron: POLL })).outcome, "ok");
    const sessions = await named.worker.getDurableObjectNamespace("SESSIONS");
    assert.deepEqual(await named.worker.listDurableObjectIds("SESSIONS"), [sessions.idFromName("held").toString()]);
    await heard(named.said, POLLED);
  });
  const alone = shipped(D1, { durableObjects: {} });
  await within(alone.worker, async () => {
    const scheduled = await alone.worker.getWorker();
    assert.equal((await scheduled.scheduled({ cron: POLL })).outcome, "ok");
    await heard(alone.said, POLLED);
  });
});
