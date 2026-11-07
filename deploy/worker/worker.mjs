import { DurableObject } from "cloudflare:workers";
import module from "../pkg/herald_wasm_bg.wasm";
import { Herald, answered, fetching, load, rounds } from "../host.mjs";
import files from "./files.mjs";
import config from "./herald.toml";

function d1(db) {
  return {
    async transaction(statements) {
      const results = await db.batch(statements.map((statement) => db.prepare(statement.sql).bind(...statement.params)));
      return results.map((result) => ({
        rows: (result.results ?? []).map((row) => Object.values(row)),
        changed: result.meta?.changes ?? 0,
      }));
    },
  };
}

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

function host(env, storage) {
  return {
    vars: Object.fromEntries(Object.entries(env).filter(([, value]) => typeof value === "string")),
    secret: (name) => (typeof env[name] === "string" ? env[name] : undefined),
    fetch: fetching(),
    async sql(at) {
      const bound = env[at.binding];
      if (!bound) throw new Error(`the Worker has no binding ${at.binding}`);
      if (at.kind === "d1") return d1(bound);
      if (at.kind === "durable-object") return storage ?? bound.get(bound.idFromName(at.object));
      throw new Error(`a Worker offers no ${at.kind} database`);
    },
  };
}

let composing;

function composed(env) {
  composing ??= load(module)
    .then(() => Herald.compose(host(env), config, files, "worker"))
    .catch((error) => {
      composing = undefined;
      throw error;
    });
  return composing;
}

const HOLD_AGAIN = 60_000;

export default {
  async fetch(request, env, ctx) {
    const { response, work } = await answered(await composed(env), request);
    if (work) ctx.waitUntil(work);
    return response;
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
    this.holding ??= load(module)
      .then(() => Herald.compose(host(this.env, this), config, files, "worker"))
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
    const sql = this.ctx.storage.sql;
    return this.ctx.storage.transactionSync(() =>
      statements.map((statement) => {
        const cursor = sql.exec(statement.sql, ...statement.params);
        const rows = cursor.raw().toArray();
        return { rows, changed: cursor.rowsWritten };
      }),
    );
  }
}
