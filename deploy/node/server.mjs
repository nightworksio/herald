import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { Herald, TIMEOUT, answering, denoKv, load, rounds, said } from "../host.mjs";
import { postgres, sqlite } from "../databases.mjs";
import { guarded } from "../vetted.mjs";

export function host(env = process.env, directory = process.cwd()) {
  return {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret: (name) => env[name],
    fetch: guarded(Number(env.HERALD_TIMEOUT_MS ?? TIMEOUT)),
    async sql(at) {
      if (at.kind === "sqlite") return sqlite(join(directory, at.file));
      if (at.kind === "postgres") return postgres(at.url);
      throw new Error(`Node offers no ${at.kind} database`);
    },
    async kv(at) {
      if (at.kind !== "deno-kv") throw new Error(`Node offers no ${at.kind} store`);
      const { openKv } = await import("@deno/kv");
      return denoKv(await openKv(at.path));
    },
  };
}

export async function serve({ env = process.env, path = env.HERALD_CONFIG ?? "herald.toml", listen = env.HERALD_LISTEN ?? "127.0.0.1:8080" } = {}) {
  await load(await readFile(new URL("../pkg/herald_wasm_bg.wasm", import.meta.url)));
  const directory = dirname(path);
  const config = await readFile(path, "utf8");
  const files = (name) => {
    try {
      return readFileSync(join(directory, name), "utf8");
    } catch {
      return undefined;
    }
  };
  const herald = await Herald.compose(host(env, directory), config, files, "node");
  const schedule = herald.schedule();
  const timers = ["poll", "housekeep"]
    .filter((round) => schedule[round])
    .map((round) => setInterval(() => rounds(herald, [round]).catch(console.error), schedule[round]));
  const [hostname, port] = [listen.slice(0, listen.lastIndexOf(":")), Number(listen.slice(listen.lastIndexOf(":") + 1))];
  const server = createServer(async (incoming, outgoing) => {
    const chunks = [];
    for await (const chunk of incoming) chunks.push(chunk);
    const [path, query = ""] = incoming.url.split(/\?(.*)/s);
    const { response, work } = await answering(herald, {
      method: incoming.method,
      path,
      query,
      headers: Object.entries(incoming.headers).flatMap(([name, value]) => [value].flat().map((each) => [name, String(each)])),
      body: Buffer.concat(chunks),
    });
    work?.catch(console.error);
    outgoing.writeHead(response.status, Object.fromEntries(response.headers));
    outgoing.end(Buffer.from(await response.arrayBuffer()));
  });
  await new Promise((resolve) => server.listen(port, hostname, resolve));
  said([`herald serves on ${hostname}:${server.address().port}`]);
  return {
    server,
    close: () => {
      for (const timer of timers) clearInterval(timer);
      return new Promise((resolve) => server.close(resolve));
    },
  };
}

if (import.meta.url === `file://${process.argv[1]}`) await serve();
