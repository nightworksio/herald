import init, { Herald, publicAddress } from "./pkg/herald_wasm.js";

export { Herald, publicAddress };

let loading;

export function load(module) {
  loading ??= init({ module_or_path: module });
  return loading;
}

export const TIMEOUT = 10_000;

export async function capped(response, most) {
  const larger = () => new Error(`the response is larger than ${most} bytes`);
  if (Number(response.headers.get("content-length") ?? 0) > most) throw larger();
  if (!response.body) return new Uint8Array();
  const reader = response.body.getReader();
  const chunks = [];
  let read = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    read += value.length;
    if (read > most) {
      await reader.cancel();
      throw larger();
    }
    chunks.push(value);
  }
  const body = new Uint8Array(read);
  let at = 0;
  for (const chunk of chunks) {
    body.set(chunk, at);
    at += chunk.length;
  }
  return body;
}

export function fetching(fetcher = globalThis.fetch, timeout = TIMEOUT) {
  return async (asked) => {
    const response = await fetcher(asked.url, {
      method: asked.method,
      headers: asked.headers,
      body: ["GET", "HEAD"].includes(asked.method) ? undefined : asked.body,
      redirect: "manual",
      signal: AbortSignal.timeout(timeout),
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

export async function answered(herald, request) {
  const url = new URL(request.url);
  const answer = await herald.answer(
    {
      method: request.method,
      path: url.pathname,
      query: url.search.slice(1),
      headers: [...request.headers],
      body: new Uint8Array(await request.arrayBuffer()),
    },
    false,
  );
  const response = new Response(answer.body.length ? answer.body : null, {
    status: answer.status,
    headers: { "content-type": answer.kind },
  });
  return { response, work: answer.work?.then(said) };
}

export async function rounds(herald, rounds = ["poll", "housekeep"]) {
  for (const round of rounds) said(await herald[round]());
}

export function said(lines) {
  for (const line of lines ?? []) console.log(line);
}
