import assert from "node:assert/strict";
import { mkdtemp, rename, writeFile } from "node:fs/promises";
import { registerHooks } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const url = process.env.HERALD_TEST_POSTGRES;
const POSTGRES = 'kind = "postgres"\nurl = { secret = "HERALD_TEST_POSTGRES" }';
const POLLED = /^news: stopped: .*\. The next poll asks again\.$/;
const VERCEL = `data:text/javascript,${encodeURIComponent("export function waitUntil() {}")}`;

registerHooks({
  resolve(specifier, context, next) {
    if (specifier === "@vercel/functions") return { url: VERCEL, shortCircuit: true };
    if (specifier === "pg") return next(specifier, { ...context, parentURL: import.meta.url });
    return next(specifier, context);
  },
});

async function configured(store) {
  const directory = await mkdtemp(join(tmpdir(), "herald-serverless-"));
  const path = join(directory, "herald.toml");
  await writeFile(
    path,
    `version = 1\n[store]\n${store}\n[sources.news]\nkind = "feed"\nurl = "https://feed.invalid/feed.xml"\n`,
  );
  return path;
}

async function within(env, work) {
  const saved = { ...process.env };
  Object.assign(process.env, env);
  try {
    return await work();
  } finally {
    for (const name of Object.keys(env)) delete process.env[name];
    Object.assign(process.env, saved);
  }
}

function asked(path, { method = "GET", secret } = {}) {
  const headers = secret ? { authorization: `Bearer ${secret}` } : {};
  return new Request(`https://herald.example${path}`, { method, headers });
}

async function answered(response) {
  return [response.status, await response.text()];
}

test("CI runs the serverless rounds against its Postgres service", () => {
  assert.ok(url || !process.env.CI, "HERALD_TEST_POSTGRES is not set");
});

test("Vercel answers, and refuses a round its cron asks for without the secret", async () => {
  const env = { HERALD_CONFIG: await configured(POSTGRES), CRON_SECRET: "cron" };
  await within(env, async () => {
    const { default: vercel } = await import("../vercel/api/herald.mjs?unheld");
    assert.deepEqual(await answered(await vercel.fetch(asked("/healthz"))), [200, "herald is up"]);
    assert.deepEqual(await answered(await vercel.fetch(asked("/herald/round/poll"))), [
      401,
      "herald does not take this request",
    ]);
    assert.deepEqual(await answered(await vercel.fetch(asked("/herald/round/poll", { secret: "other" }))), [
      401,
      "herald does not take this request",
    ]);
  });
});

test("Vercel's cron, asking with GET and its CRON_SECRET, runs a round", { skip: !url }, async () => {
  const env = { HERALD_CONFIG: await configured(POSTGRES), CRON_SECRET: "cron" };
  await within(env, async () => {
    const { default: vercel } = await import("../vercel/api/herald.mjs?cron");
    for (const method of ["GET", "POST"]) {
      assert.deepEqual(
        await answered(await vercel.fetch(asked("/herald/round/housekeep", { method, secret: "cron" }))),
        [200, "the round ran"],
        method,
      );
    }
  });
});

test("Vercel takes a round secret in place of CRON_SECRET", { skip: !url }, async () => {
  const env = { HERALD_CONFIG: await configured(POSTGRES), CRON_SECRET: "cron", HERALD_ROUND_SECRET: "own" };
  await within(env, async () => {
    const { default: vercel } = await import("../vercel/api/herald.mjs?own");
    const round = (secret) => vercel.fetch(asked("/herald/round/housekeep", { secret }));
    assert.deepEqual(await answered(await round("cron")), [401, "herald does not take this request"]);
    assert.deepEqual(await answered(await round("own")), [200, "the round ran"]);
  });
});

test("Vercel composes herald again once a failed composition can succeed", { skip: !url }, async () => {
  const written = await configured(POSTGRES);
  const path = join(dirname(written), "later.toml");
  await within({ HERALD_CONFIG: path }, async () => {
    const { default: vercel } = await import("../vercel/api/herald.mjs?later");
    await assert.rejects(vercel.fetch(asked("/healthz")), { code: "ENOENT" });
    await rename(written, path);
    assert.deepEqual(await answered(await vercel.fetch(asked("/healthz"))), [200, "herald is up"]);
  });
});

test("Netlify answers, runs a round asked for with its secret, and its schedules run the round each names", { skip: !url }, async (t) => {
  const env = { HERALD_CONFIG: await configured(POSTGRES), HERALD_ROUND_SECRET: "own" };
  await within(env, async () => {
    const { default: netlify, config } = await import("../netlify/functions/herald.mjs");
    const { default: poll, config: polling } = await import("../netlify/functions/poll.mjs");
    const { default: housekeep, config: housekeeping } = await import("../netlify/functions/housekeep.mjs");
    assert.deepEqual([config, polling, housekeeping], [
      { path: "/*" },
      { schedule: "*/5 * * * *" },
      { schedule: "17 * * * *" },
    ]);
    const context = { waitUntil() {} };
    assert.deepEqual(await answered(await netlify(asked("/healthz"), context)), [200, "herald is up"]);
    assert.deepEqual(await answered(await netlify(asked("/herald/round/housekeep", { method: "POST", secret: "own" }), context)), [
      200,
      "the round ran",
    ]);
    const logged = t.mock.method(console, "log", () => {});
    await housekeep();
    assert.equal(logged.mock.callCount(), 0);
    await poll();
    assert.equal(logged.mock.callCount(), 1);
    assert.match(logged.mock.calls[0].arguments[0], POLLED);
  });
});
