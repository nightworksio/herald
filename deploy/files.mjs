import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { Herald, load } from "./host.mjs";

const WASM = new URL("./pkg/herald_wasm_bg.wasm", import.meta.url);

export function beside(config) {
  const directory = dirname(config);
  return (name) => {
    try {
      return readFileSync(resolve(directory, name), "utf8");
    } catch {
      return undefined;
    }
  };
}

export async function composedFrom(path, host, hosting) {
  await load(await readFile(WASM));
  return Herald.compose(host, await readFile(path, "utf8"), beside(path), hosting);
}
