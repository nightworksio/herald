import { DurableObject } from "cloudflare:workers";
import wasm from "../pkg-conformance/herald_wasm_bg.wasm";
import init, { conformance } from "../pkg-conformance/herald_wasm.js";
import { transacted } from "../../worker/sql.mjs";

export class Kept extends DurableObject {
  async transaction(statements) {
    return transacted(this.ctx.storage, statements);
  }

  async fetch() {
    await init({ module_or_path: wasm });
    try {
      await conformance(this);
      return new Response("conforms");
    } catch (error) {
      return new Response(String(error?.message ?? error), { status: 500 });
    }
  }
}

export default {
  fetch(request, env) {
    return env.KEPT.get(env.KEPT.idFromName("conformance")).fetch(request);
  },
};
