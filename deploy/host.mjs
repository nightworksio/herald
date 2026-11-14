import init from "./pkg/herald_wasm.js";

export { Herald, publicAddress } from "./pkg/herald_wasm.js";

let loading;

export function load(module) {
  loading ??= init({ module_or_path: module });
  return loading;
}

export const TIMEOUT = 30_000;

export const ROUNDS = ["poll", "housekeep"];

export const SCHEDULES = { poll: "*/5 * * * *", housekeep: "17 * * * *" };

const CONFIG = "herald.toml";

export function configPath(env) {
  return env.HERALD_CONFIG ?? CONFIG;
}

export function envHost(env, offered) {
  const { by, fetch, sql, kv, secret = (name) => (typeof env[name] === "string" ? env[name] : undefined) } = offered;
  const host = {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret,
    fetch,
  };
  if (sql) host.sql = offering(by, "database", sql);
  if (kv) host.kv = offering(by, "store", kv);
  return host;
}

function offering(by, kept, kinds) {
  return async (at) => {
    if (!Object.hasOwn(kinds, at.kind)) throw new Error(`${by} offers no ${at.kind} ${kept}`);
    return kinds[at.kind](at);
  };
}

export function composeOnce(compose) {
  let composing;
  return (...args) => {
    composing ??= Promise.resolve()
      .then(() => compose(...args))
      .catch((error) => {
        composing = undefined;
        throw error;
      });
    return composing;
  };
}

export async function capped(response, most) {
  const larger = () => new Error(`the response is larger than ${most} bytes`);
  if (Number(response.headers.get("content-length") ?? 0) > most) throw larger();
  const body = await upTo(response.body, most);
  if (body.length > most) throw larger();
  return body;
}

export function fetching(fetcher = globalThis.fetch) {
  return async (asked) => {
    const response = await fetcher(asked.url, {
      method: asked.method,
      headers: asked.headers,
      body: ["GET", "HEAD"].includes(asked.method) ? undefined : asked.body,
      redirect: "manual",
      signal: AbortSignal.timeout(asked.timeout ?? TIMEOUT),
    });
    return {
      status: response.status,
      headers: [...response.headers],
      body: await capped(response, asked.most),
    };
  };
}

export function denoKv(kv) {
  return {
    async get(key) {
      const found = await kv.get(key);
      return found.versionstamp === null
        ? null
        : { value: found.value, versionstamp: found.versionstamp };
    },
    async setIf(key, value, expected) {
      const set = await kv.atomic().check({ key, versionstamp: expected }).set(key, value).commit();
      return set.ok ? set.versionstamp : null;
    },
    async deleteIf(key, expected) {
      const deleted = await kv.atomic().check({ key, versionstamp: expected }).delete(key).commit();
      return deleted.ok;
    },
    async list(prefix) {
      const pairs = [];
      for await (const entry of kv.list({ prefix })) pairs.push({ key: entry.key, value: entry.value });
      return pairs;
    },
  };
}

export async function answering(herald, asked) {
  const answer = await herald.answer(asked, false);
  if (answer.refused) console.warn(answer.refused);
  said(answer.said);
  const headers = { "content-type": answer.kind };
  if (answer.retryAfter !== undefined) headers["retry-after"] = String(answer.retryAfter);
  const response = new Response(answer.body.length ? answer.body : null, { status: answer.status, headers });
  return { response, work: answer.work?.then(said) };
}

export async function upTo(chunks, most) {
  const read = [];
  let length = 0;
  for await (const chunk of chunks ?? []) {
    read.push(chunk);
    length += chunk.length;
    if (length > most) break;
  }
  const body = new Uint8Array(length);
  let at = 0;
  for (const chunk of read) {
    body.set(chunk, at);
    at += chunk.length;
  }
  return body;
}

export async function answered(herald, request, waitUntil = (work) => work.catch(console.error)) {
  const url = new URL(request.url);
  const { response, work } = await answering(herald, {
    method: request.method,
    path: url.pathname,
    query: url.search.slice(1),
    headers: [...request.headers],
    body: await upTo(request.body, herald.limits().body),
  });
  if (work) waitUntil(work);
  return response;
}

export async function rounds(herald, names = ROUNDS) {
  const lines = await Promise.all(names.map((round) => herald[round]()));
  lines.forEach(said);
}

export function headerPairs(headers) {
  return Object.entries(headers).flatMap(([name, value]) => [value].flat().map((each) => [name, String(each)]));
}

export function said(lines) {
  for (const line of lines ?? []) console.log(line);
}
