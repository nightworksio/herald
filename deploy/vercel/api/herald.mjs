import { timingSafeEqual } from "node:crypto";
import { readFile } from "node:fs/promises";
import { waitUntil } from "@vercel/functions";
import { Herald, answered, load, rounds } from "../../host.mjs";
import { postgres } from "../../databases.mjs";
import { guarded } from "../../vetted.mjs";

const ROUNDS = "/api/herald/rounds/";

function host(env) {
  return {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret: (name) => env[name],
    fetch: guarded(),
    async sql(at) {
      if (at.kind === "postgres") return postgres(at.url);
      throw new Error(`this function offers no ${at.kind} database`);
    },
  };
}

function scheduled(given) {
  const secret = process.env.CRON_SECRET;
  if (!secret || !given) return false;
  const [expected, offered] = [Buffer.from(`Bearer ${secret}`), Buffer.from(given)];
  return expected.length === offered.length && timingSafeEqual(expected, offered);
}

let composing;

function composed() {
  composing ??= (async () => {
    await load(await readFile(new URL("../../pkg/herald_wasm_bg.wasm", import.meta.url)));
    const config = await readFile(process.env.HERALD_CONFIG ?? "herald.toml", "utf8");
    return Herald.compose(host(process.env), config, {}, "vercel");
  })().catch((error) => {
    composing = undefined;
    throw error;
  });
  return composing;
}

export default {
  async fetch(request) {
    const herald = await composed();
    const { pathname } = new URL(request.url);
    if (pathname.startsWith(ROUNDS)) {
      if (!scheduled(request.headers.get("authorization"))) return new Response(null, { status: 401 });
      await rounds(herald, [pathname.slice(ROUNDS.length)].filter((round) => ["poll", "housekeep"].includes(round)));
      return new Response(null, { status: 204 });
    }
    const { response, work } = await answered(herald, request);
    if (work) waitUntil(work);
    return response;
  },
};
