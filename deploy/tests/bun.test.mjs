import { expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { sqlite } from "../databases.mjs";
import { serve } from "../node/server.mjs";
import init, { sqlConformance } from "./pkg-conformance/herald_wasm.js";

const SECRET = "local-round-secret";

test("Bun's own SQLite passes the database suite", async () => {
  await init({ module_or_path: await readFile(new URL("pkg-conformance/herald_wasm_bg.wasm", import.meta.url)) });
  const directory = await mkdtemp(join(tmpdir(), "herald-bun-"));
  await sqlConformance(await sqlite(join(directory, "herald.sqlite"), 300), "sqlite");
});

test("herald serves on Bun, keeping its records in a SQLite file, and refuses a body past [server] body", async () => {
  const directory = await mkdtemp(join(tmpdir(), "herald-bun-"));
  const path = join(directory, "herald.toml");
  await writeFile(path, 'version = 1\n[store]\nkind = "sqlite"\n[server]\nbody = "4KiB"\n');
  const running = await serve({ env: { HERALD_ROUND_SECRET: SECRET }, path, listen: "127.0.0.1:0" });
  const base = `http://127.0.0.1:${running.server.address().port}`;
  const answered = async (response) => [response.status, await response.text()];
  const round = (secret) =>
    fetch(`${base}/herald/round/housekeep`, { method: "POST", headers: { authorization: `Bearer ${secret}` } });
  const answers = [
    await answered(await fetch(`${base}/healthz`)),
    await answered(await round("other")),
    await answered(await round(SECRET)),
    await answered(await fetch(`${base}/inbox`, { method: "POST", body: "0".repeat(4097) })),
  ];
  await running.close();
  expect(answers).toEqual([
    [200, "herald is up"],
    [401, "herald does not take this request"],
    [200, "the round ran"],
    [413, "herald does not take this request"],
  ]);
  const kept = new Database(join(directory, "herald.sqlite"));
  expect(kept.prepare("SELECT layout FROM herald_meta").get()).toEqual({ layout: 2 });
  kept.close();
});
