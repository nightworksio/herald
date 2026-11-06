import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { before, test } from "node:test";
import { capped, fetching, load, publicAddress } from "../host.mjs";
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
