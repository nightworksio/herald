import assert from "node:assert/strict";
import { mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";
import { before, test } from "node:test";
import {
  ROUNDS,
  SCHEDULES,
  answered,
  answering,
  capped,
  composeOnce,
  configPath,
  envHost,
  fetching,
  headerPairs,
  load,
  publicAddress,
  rounds,
  upTo,
} from "../host.mjs";
import { beside } from "../files.mjs";
import { serve } from "../node/server.mjs";
import { guarded, vetted } from "../vetted.mjs";

before(async () => {
  await load(await readFile(new URL("../pkg/herald_wasm_bg.wasm", import.meta.url)));
});

function listening(handler) {
  return new Promise((resolve) => {
    const server = createServer(handler);
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

test("a body is read no further than herald asks", async () => {
  const response = (text, headers = {}) => new Response(text, { headers });
  assert.deepEqual(await capped(response("ok"), 2), new TextEncoder().encode("ok"));
  await assert.rejects(capped(response("big"), 2), /larger than 2 bytes/);
  await assert.rejects(capped(response("ok", { "content-length": "9" }), 2), /larger than 2 bytes/);
  assert.deepEqual(await capped(new Response(null), 2), new Uint8Array());
});

test("a fetch follows no redirect and sends no body with a GET", async () => {
  const server = await listening((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => {
      response.writeHead(request.url === "/moved" ? 302 : 200, { location: "/elsewhere" });
      response.end(`${request.method} ${body}`);
    });
  });
  const base = `http://127.0.0.1:${server.address().port}`;
  const send = fetching();
  const moved = await send({ method: "GET", url: `${base}/moved`, headers: [], body: new Uint8Array([1]), most: 100 });
  assert.equal(moved.status, 302);
  assert.equal(new TextDecoder().decode(moved.body), "GET ");
  const posted = await send({ method: "POST", url: `${base}/`, headers: [["x-a", "1"]], body: new TextEncoder().encode("hi"), most: 100 });
  assert.equal(new TextDecoder().decode(posted.body), "POST hi");
  server.close();
});

test("a name is reached on others' behalf only where every address it resolves to is public", async () => {
  assert.equal(publicAddress("1.1.1.1"), true);
  assert.equal(publicAddress("127.0.0.1"), false);
  const refused = await new Promise((resolve) => vetted("localhost", {}, (error) => resolve(error)));
  assert.match(refused.message, /not public/);
  const send = guarded();
  await assert.rejects(
    send({ method: "GET", url: "https://localhost/", headers: [], body: new Uint8Array(), most: 10, public: true }),
    /not public/,
  );
});

test("herald serves on Node, keeping its records in a SQLite file", async () => {
  const directory = await mkdtemp(join(tmpdir(), "herald-node-"));
  const path = join(directory, "herald.toml");
  await writeFile(path, 'version = 1\n[store]\nkind = "sqlite"\n');
  const running = await serve({ env: { HERALD_ROUND_SECRET: "local" }, path, listen: "127.0.0.1:0" });
  const { port } = running.server.address();
  const health = await fetch(`http://127.0.0.1:${port}/healthz`);
  const unknown = await fetch(`http://127.0.0.1:${port}/nowhere`);
  const round = await fetch(`http://127.0.0.1:${port}/herald/round/housekeep`, {
    method: "POST",
    headers: { authorization: "Bearer local" },
  });
  const answers = [
    [health.status, await health.text()],
    [unknown.status, await unknown.text()],
    [round.status, await round.text()],
  ];
  await running.close();
  assert.deepEqual(answers, [
    [200, "herald is up"],
    [404, "herald serves no such path here"],
    [200, "the round ran"],
  ]);
  const kept = new DatabaseSync(join(directory, "herald.sqlite"));
  assert.equal(kept.prepare("SELECT layout FROM herald_meta").get().layout, 2);
  kept.close();
});

test("Node listens where the config says, an IPv6 address too, and keeps an absolute store path", async () => {
  const directory = await mkdtemp(join(tmpdir(), "herald-node-"));
  const elsewhere = await mkdtemp(join(tmpdir(), "herald-store-"));
  const file = join(elsewhere, "kept.db");
  const path = join(directory, "herald.toml");
  await writeFile(path, `version = 1\n[store]\nkind = "sqlite"\npath = ${JSON.stringify(file)}\n[server]\nlisten = "[::1]:0"\n`);
  const running = await serve({ env: {}, path });
  const { address, port } = running.server.address();
  assert.equal(address, "::1");
  const health = await fetch(`http://[::1]:${port}/healthz`);
  assert.equal(health.status, 200);
  await running.close();
  assert.ok((await stat(file)).isFile());
});

test("a config's files are read beside it, an absolute name as itself", async () => {
  const directory = await mkdtemp(join(tmpdir(), "herald-files-"));
  await writeFile(join(directory, "post.md"), "beside");
  const elsewhere = join(await mkdtemp(join(tmpdir(), "herald-files-")), "away.md");
  await writeFile(elsewhere, "away");
  const files = beside(join(directory, "herald.toml"));
  assert.equal(files("post.md"), "beside");
  assert.equal(files(elsewhere), "away");
  assert.equal(files("absent.md"), undefined);
});

test("a request aborted mid-body is dropped and the next is answered", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "herald-node-"));
  const path = join(directory, "herald.toml");
  await writeFile(path, 'version = 1\n[store]\nkind = "sqlite"\n');
  const warned = t.mock.method(console, "warn", () => {});
  const running = await serve({ env: {}, path, listen: "127.0.0.1:0" });
  const { port } = running.server.address();
  await new Promise((resolve, reject) => {
    const socket = connect(port, "127.0.0.1", () => {
      socket.write("POST /inbox HTTP/1.1\r\nHost: herald.example\r\nContent-Length: 100\r\n\r\npart");
      setTimeout(() => socket.destroy(), 50);
    });
    socket.on("close", resolve);
    socket.on("error", reject);
  });
  while (warned.mock.callCount() === 0) await new Promise((resolve) => setTimeout(resolve, 10));
  assert.match(warned.mock.calls[0].arguments[0], /^herald dropped POST \/inbox: its body did not arrive/);
  const health = await fetch(`http://127.0.0.1:${port}/healthz`);
  assert.equal(health.status, 200);
  await running.close();
});

test("a body is read no further than one byte past the most herald takes", async () => {
  const chunks = [new Uint8Array([1, 2]), new Uint8Array([3, 4]), new Uint8Array([5])];
  assert.deepEqual(await upTo(chunks, 3), new Uint8Array([1, 2, 3, 4]));
  assert.deepEqual(await upTo(chunks, 9), new Uint8Array([1, 2, 3, 4, 5]));
  assert.deepEqual(await upTo(null, 3), new Uint8Array());
});

test("a refusal that asks its sender to wait says how long", async (t) => {
  t.mock.method(console, "warn", () => {});
  const herald = {
    answer: async () => ({ status: 503, kind: "text/plain", body: new Uint8Array(), retryAfter: 30, refused: "busy" }),
  };
  const { response } = await answering(herald, {});
  assert.equal(response.status, 503);
  assert.equal(response.headers.get("retry-after"), "30");
});

test("a body larger than [server] body is refused on Node", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "herald-node-"));
  const path = join(directory, "herald.toml");
  await writeFile(path, 'version = 1\n[store]\nkind = "sqlite"\n[server]\nbody = "4KiB"\n');
  t.mock.method(console, "warn", () => {});
  const running = await serve({ env: {}, path, listen: "127.0.0.1:0" });
  const { port } = running.server.address();
  const refused = await fetch(`http://127.0.0.1:${port}/inbox`, { method: "POST", body: "0".repeat(4097) });
  assert.equal(refused.status, 413);
  assert.equal(refused.headers.get("connection"), "close");
  assert.equal(await refused.text(), "herald does not take this request");
  const taken = await fetch(`http://127.0.0.1:${port}/inbox`, { method: "POST", body: "0".repeat(4096) });
  const answer = [taken.status, taken.headers.get("connection"), await taken.text()];
  await running.close();
  assert.deepEqual(answer, [404, "keep-alive", "herald serves no actor on this host here"]);
});

test("a host offers the environment's strings as its variables and secrets, and only the databases it names", () => {
  const fetch = async () => ({});
  const host = envHost({ NAME: "set", COUNT: 3, BOUND: {} }, { by: "Here", fetch });
  assert.deepEqual(host.vars, { NAME: "set" });
  assert.deepEqual(["NAME", "COUNT", "BOUND", "toString", "ABSENT"].map(host.secret), ["set", undefined, undefined, undefined, undefined]);
  assert.equal(host.fetch, fetch);
  assert.equal("sql" in host, false);
  assert.equal("kv" in host, false);
});

test("a host opens each database by its kind and refuses a kind it does not offer, naming itself", async () => {
  const host = envHost(
    {},
    {
      by: "Here",
      sql: {
        lite: async (at) => `lite at ${at.file}`,
        broken: () => {
          throw new Error("no binding");
        },
      },
      kv: { "deno-kv": async (at) => `kv at ${at.path}` },
    },
  );
  assert.equal(await host.sql({ kind: "lite", file: "f" }), "lite at f");
  assert.equal(await host.kv({ kind: "deno-kv", path: "p" }), "kv at p");
  await assert.rejects(host.sql({ kind: "broken" }), /^Error: no binding$/);
  await assert.rejects(host.sql({ kind: "postgres" }), /^Error: Here offers no postgres database$/);
  await assert.rejects(host.sql({ kind: "toString" }), /^Error: Here offers no toString database$/);
  await assert.rejects(host.kv({ kind: "fastly-kv" }), /^Error: Here offers no fastly-kv store$/);
});

test("a host takes its own secret lookup in place of the environment's", () => {
  const host = envHost({ OWN: "env" }, { by: "Here", secret: (name) => `looked up ${name}` });
  assert.equal(host.secret("OWN"), "looked up OWN");
});

test("herald is composed once, from the first call's arguments, and again after a composition fails", async () => {
  const asked = [];
  let failing = true;
  const composed = composeOnce(async (...args) => {
    asked.push(args);
    if (failing) throw new Error("not yet");
    return { composed: args };
  });
  await assert.rejects(composed("first"), /not yet/);
  failing = false;
  const [one, two] = await Promise.all([composed("second"), composed("third")]);
  assert.equal(one, two);
  assert.deepEqual(one, { composed: ["second"] });
  assert.equal(await composed("fourth"), one);
  assert.deepEqual(asked, [["first"], ["second"]]);
});

test("a composition that throws before it awaits is refused and tried again", async () => {
  let tries = 0;
  const composed = composeOnce(() => {
    tries += 1;
    if (tries === 1) throw new Error("at once");
    return "composed";
  });
  await assert.rejects(composed(), /at once/);
  assert.equal(await composed(), "composed");
  assert.equal(tries, 2);
});

test("the config is read from HERALD_CONFIG, or herald.toml where it is unset", () => {
  assert.equal(configPath({}), "herald.toml");
  assert.equal(configPath({ HERALD_CONFIG: "/etc/herald/herald.toml" }), "/etc/herald/herald.toml");
});

test("an answer hands the work it leaves to the host, which logs a failure where it is not told how to wait", async (t) => {
  const herald = (work) => ({
    limits: () => ({ body: 16 }),
    answer: async (asked) => ({ status: 202, kind: "text/plain", body: asked.body, work }),
  });
  const request = () => new Request("https://herald.example/inbox?a=1", { method: "POST", body: "hi" });
  const waited = [];
  const response = await answered(herald(Promise.resolve([])), request(), (work) => waited.push(work));
  assert.equal(response.status, 202);
  assert.equal(await response.text(), "hi");
  assert.equal(waited.length, 1);
  await waited[0];
  assert.equal((await answered(herald(undefined), request(), (work) => waited.push(work))).status, 202);
  assert.equal(waited.length, 1);
  const logged = t.mock.method(console, "error", () => {});
  await answered(herald(Promise.reject(new Error("lost"))), request());
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(logged.mock.callCount(), 1);
  assert.match(String(logged.mock.calls[0].arguments[0]), /lost/);
});

test("both rounds run where none is named", async (t) => {
  t.mock.method(console, "log", () => {});
  const ran = [];
  const herald = Object.fromEntries(
    ROUNDS.map((round) => [
      round,
      async () => {
        ran.push(round);
        return [];
      },
    ]),
  );
  await rounds(herald);
  assert.deepEqual(ran, ["poll", "housekeep"]);
});

test("a Node header given more than once is one pair per value", () => {
  assert.deepEqual(headerPairs({ host: "herald.example", "set-cookie": ["a=1", "b=2"], "content-length": 2 }), [
    ["host", "herald.example"],
    ["set-cookie", "a=1"],
    ["set-cookie", "b=2"],
    ["content-length", "2"],
  ]);
});

test("every template's own schedule asks for the rounds when the shared schedule does", async () => {
  const read = (path) => readFile(new URL(`../${path}`, import.meta.url), "utf8");
  const { poll, housekeep } = SCHEDULES;
  const vercel = JSON.parse(await read("vercel/vercel.json")).crons.map((cron) => cron.schedule);
  assert.deepEqual(vercel, [poll, housekeep]);
  const wrangler = await read("worker/wrangler.toml");
  assert.ok(wrangler.includes(`crons = ["${poll}", "${housekeep}"]`), wrangler);
  assert.ok(wrangler.includes(`HERALD_HOUSEKEEP_CRON = "${housekeep}"`), wrangler);
  for (const [round, schedule] of Object.entries(SCHEDULES)) {
    assert.ok((await read(`netlify/functions/${round}.mjs`)).includes(`schedule: "${schedule}"`), round);
    const azure = JSON.parse(await read(`azure/${round}/function.json`)).bindings[0].schedule;
    assert.equal(azure, `0 ${schedule}`);
  }
});
