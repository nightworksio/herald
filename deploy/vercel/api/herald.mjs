import { readFile } from "node:fs/promises";
import { waitUntil } from "@vercel/functions";
import { Herald, answered, load } from "../../host.mjs";
import { postgres } from "../../databases.mjs";
import { beside } from "../../files.mjs";
import { guarded } from "../../vetted.mjs";

const ROUNDS = "/herald/round/";

function host(env) {
  return {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret: (name) => (name === "HERALD_ROUND_SECRET" ? env[name] || env.CRON_SECRET : env[name]),
    fetch: guarded(),
    async sql(at) {
      if (at.kind === "postgres") return postgres(at);
      throw new Error(`this function offers no ${at.kind} database`);
    },
  };
}

let composing;

function composed() {
  composing ??= (async () => {
    await load(await readFile(new URL("../../pkg/herald_wasm_bg.wasm", import.meta.url)));
    const path = process.env.HERALD_CONFIG ?? "herald.toml";
    return Herald.compose(host(process.env), await readFile(path, "utf8"), beside(path), "vercel");
  })().catch((error) => {
    composing = undefined;
    throw error;
  });
  return composing;
}

export default {
  async fetch(request) {
    const herald = await composed();
    const scheduled = request.method === "GET" && new URL(request.url).pathname.startsWith(ROUNDS);
    const asked = scheduled ? new Request(request, { method: "POST" }) : request;
    const { response, work } = await answered(herald, asked);
    if (work) waitUntil(work);
    return response;
  },
};
