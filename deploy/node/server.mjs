import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, resolve as absolute } from "node:path";
import { Herald, TIMEOUT, answering, denoKv, load, rounds, said } from "../host.mjs";
import { postgres, sqlite } from "../databases.mjs";
import { beside } from "../files.mjs";
import { guarded } from "../vetted.mjs";

export function host(env = process.env, directory = process.cwd()) {
  return {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret: (name) => env[name],
    fetch: guarded(Number(env.HERALD_TIMEOUT_MS ?? TIMEOUT)),
    async sql(at) {
      if (at.kind === "sqlite") return sqlite(absolute(directory, at.file), at.busy);
      if (at.kind === "postgres") return postgres(at);
      throw new Error(`Node offers no ${at.kind} database`);
    },
    async kv(at) {
      if (at.kind !== "deno-kv") throw new Error(`Node offers no ${at.kind} store`);
      const { openKv } = await import("@deno/kv");
      return denoKv(await openKv(at.path));
    },
  };
}

function bodyOf(incoming, most) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let length = 0;
    const taken = (chunk) => {
      chunks.push(chunk);
      length += chunk.length;
      if (length > most) {
        incoming.off("data", taken);
        incoming.pause();
        resolve(Buffer.concat(chunks));
      }
    };
    incoming.on("data", taken);
    incoming.on("end", () => resolve(Buffer.concat(chunks)));
    incoming.on("error", reject);
    incoming.on("close", () => {
      if (!incoming.complete) reject(new Error("the request was aborted"));
    });
  });
}

export async function serve({ env = process.env, path = env.HERALD_CONFIG ?? "herald.toml", listen = env.HERALD_LISTEN } = {}) {
  await load(await readFile(new URL("../pkg/herald_wasm_bg.wasm", import.meta.url)));
  const directory = dirname(path);
  const config = await readFile(path, "utf8");
  const herald = await Herald.compose(host(env, directory), config, beside(path), "node");
  const schedule = herald.schedule();
  const timers = ["poll", "housekeep"]
    .filter((round) => schedule[round])
    .map((round) => setInterval(() => rounds(herald, [round]).catch(console.error), schedule[round]));
  const { hostname, port } = herald.listen(listen || undefined);
  const limits = herald.limits();
  const server = createServer(async (incoming, outgoing) => {
    const [path, query = ""] = incoming.url.split(/\?(.*)/s);
    let body;
    try {
      body = await bodyOf(incoming, limits.body);
    } catch (error) {
      console.warn(`herald dropped ${incoming.method} ${path}: its body did not arrive: ${error.message}`);
      outgoing.destroy();
      return;
    }
    try {
      const { response, work } = await answering(herald, {
        method: incoming.method,
        path,
        query,
        headers: Object.entries(incoming.headers).flatMap(([name, value]) => [value].flat().map((each) => [name, String(each)])),
        body,
      });
      if (body.length > limits.body) outgoing.setHeader("connection", "close");
      work?.catch(console.error);
      outgoing.writeHead(response.status, Object.fromEntries(response.headers));
      outgoing.end(Buffer.from(await response.arrayBuffer()));
    } catch (error) {
      console.error(`herald could not answer ${incoming.method} ${path}: ${error.message}`);
      if (outgoing.headersSent) {
        outgoing.destroy();
        return;
      }
      outgoing.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
      outgoing.end("herald cannot answer this request now");
    }
  });
  server.maxConnections = limits.connections;
  server.headersTimeout = limits.read;
  server.requestTimeout = limits.read * 2;
  server.setTimeout(limits.held);
  await new Promise((resolve) => server.listen(port, hostname, resolve));
  said([`herald serves on ${hostname.includes(":") ? `[${hostname}]` : hostname}:${server.address().port}`]);
  return {
    server,
    close: () => {
      for (const timer of timers) clearInterval(timer);
      return new Promise((resolve) => server.close(resolve));
    },
  };
}

if (import.meta.url === `file://${process.argv[1]}`) await serve();
