import { answered, configPath, denoKv, envHost, rounds } from "../host.mjs";
import { composedFrom } from "../files.mjs";
import { guarded } from "../vetted.mjs";

const env = Deno.env.toObject();
const host = envHost(env, {
  by: "Deno",
  fetch: guarded(),
  kv: { "deno-kv": async (at: { path?: string }) => denoKv(await Deno.openKv(at.path)) },
});
const herald = await composedFrom(configPath(env), host, "deno");

Deno.cron("herald poll", env.HERALD_POLL_CRON ?? "*/5 * * * *", () => rounds(herald, ["poll"]));
Deno.cron("herald housekeep", env.HERALD_HOUSEKEEP_CRON ?? "17 * * * *", () => rounds(herald, ["housekeep"]));

Deno.serve(herald.listen(env.HERALD_LISTEN || undefined), (request) => answered(herald, request));
