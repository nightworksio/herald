import { Herald, answered, denoKv, load, rounds } from "../host.mjs";
import { guarded } from "../vetted.mjs";

function host(env: Record<string, string>) {
  return {
    vars: env,
    secret: (name: string) => env[name],
    fetch: guarded(),
    async kv(at: { kind: string; path?: string }) {
      if (at.kind !== "deno-kv") throw new Error(`Deno offers no ${at.kind} store`);
      return denoKv(await Deno.openKv(at.path));
    },
  };
}

const env = Deno.env.toObject();
const path = env.HERALD_CONFIG ?? "herald.toml";
const directory = path.includes("/") ? path.slice(0, path.lastIndexOf("/") + 1) : "";
await load(await Deno.readFile(new URL("../pkg/herald_wasm_bg.wasm", import.meta.url)));
const files = (name: string) => {
  try {
    return Deno.readTextFileSync(`${directory}${name}`);
  } catch {
    return undefined;
  }
};
const herald = await Herald.compose(host(env), await Deno.readTextFile(path), files, "deno");

Deno.cron("herald poll", env.HERALD_POLL_CRON ?? "*/5 * * * *", () => rounds(herald, ["poll"]));
Deno.cron("herald housekeep", env.HERALD_HOUSEKEEP_CRON ?? "17 * * * *", () => rounds(herald, ["housekeep"]));

Deno.serve(herald.listen(env.HERALD_LISTEN || undefined), async (request) => {
  const { response, work } = await answered(herald, request);
  work?.catch(console.error);
  return response;
});
