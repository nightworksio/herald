import { answered, composeOnce, configPath, envHost } from "../../host.mjs";
import { postgres } from "../../databases.mjs";
import { composedFrom } from "../../files.mjs";
import { guarded } from "../../vetted.mjs";

function host(env) {
  return envHost(env, { by: "this function", fetch: guarded(), sql: { postgres } });
}

export const composed = composeOnce(() => composedFrom(configPath(process.env), host(process.env), "netlify"));

export default async function herald(request, context) {
  return answered(await composed(), request, (work) => context.waitUntil(work));
}

export const config = { path: "/*" };
