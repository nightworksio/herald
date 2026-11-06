import { load } from "../host.mjs";
import { guarded } from "../vetted.mjs";

Deno.test("Deno reaches a name on others' behalf only where it resolves to public addresses", async () => {
  await load(await Deno.readFile(new URL("../pkg/herald_wasm_bg.wasm", import.meta.url)));
  const send = guarded();
  let refused = "";
  try {
    await send({ method: "GET", url: "https://localhost/", headers: [], body: new Uint8Array(), most: 10, public: true });
  } catch (error) {
    refused = String(error);
  }
  if (!refused.includes("not public")) throw new Error(`a loopback name was reached: ${refused}`);
});
