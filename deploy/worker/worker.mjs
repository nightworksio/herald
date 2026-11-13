import { DurableObject } from "cloudflare:workers";
import module from "../pkg/herald_wasm_bg.wasm";
import { Herald, answered, composeOnce, envHost, fetching, load, rounds } from "../host.mjs";
import files from "./files.mjs";
import { d1, transacted } from "./sql.mjs";
import config from "./herald.toml";

function socket(webSocket) {
  const frames = [];
  const waiting = [];
  const heard = (frame) => (waiting.length ? waiting.shift()(frame) : frames.push(frame));
  webSocket.addEventListener("message", (event) => heard({ text: String(event.data) }));
  webSocket.addEventListener("close", (event) => heard({ closed: event.code || null }));
  webSocket.addEventListener("error", () => heard({ closed: null }));
  webSocket.accept();
  return {
    async send(text) {
      webSocket.send(text);
    },
    receive() {
      return frames.length ? Promise.resolve(frames.shift()) : new Promise((resolve) => waiting.push(resolve));
    },
  };
}

const sockets = {
  async open(url) {
    const answer = await fetch(url.replace(/^wss:/, "https:"), { headers: { Upgrade: "websocket" } });
    if (!answer.webSocket) throw new Error(`${new URL(url).host} answered ${answer.status}, not a WebSocket`);
    return socket(answer.webSocket);
  },
};

function bound(env, at) {
  const binding = env[at.binding];
  if (!binding) throw new Error(`the Worker has no binding ${at.binding}`);
  return binding;
}

function host(env, storage) {
  return envHost(env, {
    by: "a Worker",
    fetch: fetching(),
    sql: {
      d1: (at) => d1(bound(env, at)),
      "durable-object": (at) => {
        const objects = bound(env, at);
        return storage ?? objects.get(objects.idFromName(at.object));
      },
    },
  });
}

function compose(env, storage) {
  return load(module).then(() => Herald.compose(host(env, storage), config, files, "worker"));
}

const composed = composeOnce(compose);

const HOLD_AGAIN = 60_000;

export default {
  async fetch(request, env, ctx) {
    return answered(await composed(env), request, (work) => ctx.waitUntil(work));
  },

  async scheduled(event, env, ctx) {
    const housekeeping = event.cron === env.HERALD_HOUSEKEEP_CRON;
    ctx.waitUntil(rounds(await composed(env), [housekeeping ? "housekeep" : "poll"]));
    const holder = env[env.HERALD_SESSIONS_BINDING ?? "HERALD"];
    if (holder) ctx.waitUntil(holder.get(holder.idFromName(env.HERALD_SESSIONS_OBJECT ?? "herald")).hold());
  },
};

export class HeraldObject extends DurableObject {
  hold() {
    this.holding ??= compose(this.env, this)
      .then((herald) => herald.hold(sockets))
      .catch(console.error)
      .finally(() => {
        this.holding = undefined;
      });
    return this.ctx.storage.setAlarm(Date.now() + HOLD_AGAIN);
  }

  async alarm() {
    await this.hold();
  }

  transaction(statements) {
    return transacted(this.ctx.storage, statements);
  }
}
