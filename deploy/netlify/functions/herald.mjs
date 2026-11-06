import { readFile } from "node:fs/promises";
import { Herald, answered, load } from "../../host.mjs";
import { postgres } from "../../databases.mjs";
import { guarded } from "../../vetted.mjs";

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

let composing;

export function composed() {
  composing ??= (async () => {
    await load(await readFile(new URL("../../pkg/herald_wasm_bg.wasm", import.meta.url)));
    const config = await readFile(process.env.HERALD_CONFIG ?? "herald.toml", "utf8");
    return Herald.compose(host(process.env), config, {}, "netlify");
  })().catch((error) => {
    composing = undefined;
    throw error;
  });
  return composing;
}

export default async function herald(request, context) {
  const { response, work } = await answered(await composed(), request);
  if (work) context.waitUntil(work);
  return response;
}

export const config = { path: "/*" };
