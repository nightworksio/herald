import { load } from "../host.mjs";
import { guarded, publicly } from "../vetted.mjs";

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

Deno.test("Deno connects to the address the guard's lookup answers, not one it resolves itself", async () => {
  const listener = Deno.listen({ hostname: "127.0.0.1", port: 0 });
  const { port } = listener.addr as Deno.NetAddr;
  const accepted = listener.accept();
  const answered = (_: string, options: { all?: boolean }, callback: (...found: unknown[]) => void) =>
    options.all ? callback(null, [{ address: "127.0.0.1", family: 4 }]) : callback(null, "127.0.0.1", 4);
  const sent = publicly(`https://proof.invalid:${port}/`, { method: "GET", headers: [] }, answered).catch(() => undefined);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("nothing connected to the address the lookup answered")), 5000);
  });
  const connection = await Promise.race([accepted, late]);
  clearTimeout(timer);
  connection.close();
  listener.close();
  await sent;
});
