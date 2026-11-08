import assert from "node:assert/strict";
import { test } from "node:test";
import { Miniflare } from "miniflare";

test("a Durable Object's own SQLite storage keeps every record a store keeps", async () => {
  const worker = new Miniflare({
    modules: true,
    scriptPath: new URL("worker/conformance.mjs", import.meta.url).pathname,
    modulesRules: [
      { type: "ESModule", include: ["**/*.js", "**/*.mjs"] },
      { type: "CompiledWasm", include: ["**/*.wasm"] },
    ],
    durableObjects: { KEPT: { className: "Kept", useSQLite: true } },
    compatibilityDate: "2026-07-01",
    cf: false,
  });
  try {
    const answered = await worker.dispatchFetch("http://localhost/");
    assert.equal(await answered.text(), "conforms");
    assert.equal(answered.status, 200);
  } finally {
    await worker.dispose();
  }
});
