import { denoKv } from "../host.mjs";
import init, { kvConformance } from "./pkg-conformance/herald_wasm.js";

const SECRET = "local-round-secret";

async function up(base: string) {
  for (let tries = 0; tries < 200; tries += 1) {
    try {
      const answer = await fetch(`${base}/healthz`);
      return answer;
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  throw new Error(`${base} never answered`);
}

function unused(): number {
  const listener = Deno.listen({ hostname: "127.0.0.1", port: 0 });
  const { port } = listener.addr as Deno.NetAddr;
  listener.close();
  return port;
}

function equal(actual: unknown, expected: unknown) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

Deno.test("Deno KV passes the key-value suite", async () => {
  await init({ module_or_path: await Deno.readFile(new URL("pkg-conformance/herald_wasm_bg.wasm", import.meta.url)) });
  const kv = await Deno.openKv(":memory:");
  try {
    equal(await kvConformance(denoKv(kv)), undefined);
  } finally {
    kv.close();
  }
});

Deno.test("Deno's entry answers where it is told to listen, and runs a round asked for with its secret over the Deno KV its config names", async () => {
  const directory = await Deno.makeTempDir();
  const config = `${directory}/herald.toml`;
  const kept = `${directory}/herald.kv`;
  await Deno.writeTextFile(config, `version = 1\n[store]\nkind = "deno-kv"\npath = ${JSON.stringify(kept)}\n`);
  const base = `http://127.0.0.1:${unused()}`;
  const entry = new Deno.Command(Deno.execPath(), {
    args: [
      "run",
      "--unstable-cron",
      "--unstable-kv",
      "--allow-read",
      "--allow-write",
      "--allow-net",
      "--allow-env",
      "--allow-sys",
      new URL("../deno/main.ts", import.meta.url).pathname,
    ],
    env: { HERALD_CONFIG: config, HERALD_LISTEN: base.slice("http://".length), HERALD_ROUND_SECRET: SECRET },
    stdout: "null",
    stderr: "null",
  }).spawn();
  try {
    const health = await up(base);
    const round = (secret?: string) =>
      fetch(`${base}/herald/round/housekeep`, {
        method: "POST",
        headers: secret ? { authorization: `Bearer ${secret}` } : {},
      });
    const unheld = await round();
    const held = await round(SECRET);
    equal(
      [
        [health.status, await health.text()],
        [unheld.status, await unheld.text()],
        [held.status, await held.text()],
      ],
      [
        [200, "herald is up"],
        [401, "herald does not take this request"],
        [200, "the round ran"],
      ],
    );
    if (!(await Deno.stat(kept)).isFile) throw new Error("the round opened no Deno KV at the path the config names");
  } finally {
    entry.kill();
    await entry.status;
  }
});
