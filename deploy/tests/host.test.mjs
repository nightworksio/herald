import assert from "node:assert/strict";
import { mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { before, test } from "node:test";
import { answering, capped, fetching, load, publicAddress, upTo } from "../host.mjs";
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
  const running = await serve({ env: {}, path, listen: "127.0.0.1:0" });
  const { port } = running.server.address();
  const health = await fetch(`http://127.0.0.1:${port}/healthz`);
  assert.equal(health.status, 200);
  assert.equal(await health.text(), "herald is up");
  const unknown = await fetch(`http://127.0.0.1:${port}/nowhere`);
  assert.equal(unknown.status, 404);
  await running.close();
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
  const refused = await fetch(`http://127.0.0.1:${port}/inbox`, { method: "POST", body: "0".repeat(5000) });
  assert.equal(refused.status, 413);
  assert.equal(await refused.text(), "herald does not take this request");
  await running.close();
});
