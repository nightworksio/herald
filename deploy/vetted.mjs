import { lookup } from "node:dns";
import { request } from "node:https";
import { Readable } from "node:stream";
import { fetching, publicAddress } from "./host.mjs";

export function vetted(hostname, options, callback) {
  lookup(hostname, { all: true }, (error, addresses) => {
    if (error) return callback(error);
    if (!addresses.length || !addresses.every((found) => publicAddress(found.address))) {
      return callback(new Error(`${hostname} resolves to an address that is not public`));
    }
    if (options.all) return callback(null, addresses);
    return callback(null, addresses[0].address, addresses[0].family);
  });
}

export function publicly(url, init) {
  return new Promise((resolve, reject) => {
    const sent = request(url, { method: init.method, headers: Object.fromEntries(init.headers), lookup: vetted, signal: init.signal }, (answer) => {
      const headers = Object.entries(answer.headers).flatMap(([name, value]) => [value].flat().map((each) => [name, String(each)]));
      resolve(new Response(Readable.toWeb(answer), { status: answer.statusCode, headers }));
    });
    sent.on("error", reject);
    sent.end(init.body);
  });
}

export function guarded(timeout, fetcher = globalThis.fetch) {
  const direct = fetching(fetcher, timeout);
  const vettedly = fetching(publicly, timeout);
  return (asked) => (asked.public ? vettedly(asked) : direct(asked));
}
