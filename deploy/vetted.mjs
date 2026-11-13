import { lookup } from "node:dns";
import { request } from "node:https";
import { Readable } from "node:stream";
import { fetching, headerPairs, publicAddress } from "./host.mjs";

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

export function publicly(url, init, lookup = vetted) {
  return new Promise((resolve, reject) => {
    const sent = request(url, { method: init.method, headers: Object.fromEntries(init.headers), lookup, signal: init.signal }, (answer) => {
      resolve(new Response(Readable.toWeb(answer), { status: answer.statusCode, headers: headerPairs(answer.headers) }));
    });
    sent.on("error", reject);
    sent.end(init.body);
  });
}

export function guarded(fetcher = globalThis.fetch) {
  const direct = fetching(fetcher);
  const vettedly = fetching(publicly);
  return (asked) => (asked.public ? vettedly(asked) : direct(asked));
}
