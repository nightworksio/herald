import { DurableObject } from "cloudflare:workers";
import wasm from "../pkg-conformance/herald_wasm_bg.wasm";
import init, { conformance, sqlConformance } from "../pkg-conformance/herald_wasm.js";
import { d1, transacted } from "../../worker/sql.mjs";

const refusing = {
  transaction() {
    return Promise.reject(new Error("the database refused"));
  },
};

async function conforms(suite) {
  await init({ module_or_path: wasm });
  try {
    await suite();
    return new Response("conforms");
  } catch (error) {
    return new Response(String(error?.message ?? error), { status: 500 });
  }
}

export class Kept extends DurableObject {
  async transaction(statements) {
    return transacted(this.ctx.storage, statements);
  }

  async fetch(request) {
    const path = new URL(request.url).pathname;
    if (path === "/sql") return conforms(() => sqlConformance(this, "sqlite"));
    return conforms(() => conformance(path === "/refusing" ? refusing : this));
  }
}

export default {
  fetch(request, env) {
    if (new URL(request.url).pathname === "/d1") return conforms(() => sqlConformance(d1(env.DB), "sqlite"));
    return env.KEPT.get(env.KEPT.idFromName("conformance")).fetch(request);
  },
};
