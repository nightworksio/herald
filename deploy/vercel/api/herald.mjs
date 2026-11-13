import { waitUntil } from "@vercel/functions";
import { answered, composeOnce, configPath, envHost } from "../../host.mjs";
import { postgres } from "../../databases.mjs";
import { composedFrom } from "../../files.mjs";
import { guarded } from "../../vetted.mjs";

const ROUND_PATH = "/herald/round/";

function host(env) {
  return envHost(env, {
    by: "this function",
    secret: (name) => (name === "HERALD_ROUND_SECRET" ? env[name] || env.CRON_SECRET : env[name]),
    fetch: guarded(),
    sql: { postgres },
  });
}

const composed = composeOnce(() => composedFrom(configPath(process.env), host(process.env), "vercel"));

export default {
  async fetch(request) {
    const herald = await composed();
    const scheduled = request.method === "GET" && new URL(request.url).pathname.startsWith(ROUND_PATH);
    const asked = scheduled ? new Request(request, { method: "POST" }) : request;
    return answered(herald, asked, waitUntil);
  },
};
