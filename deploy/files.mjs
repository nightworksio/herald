import { readFileSync, realpathSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, resolve, sep } from "node:path";
import { Herald, load } from "./host.mjs";

const WASM = new URL("./pkg/herald_wasm_bg.wasm", import.meta.url);

export function beside(config) {
  const directory = dirname(config);
  return (name, within = directory) => {
    let base;
    let file;
    try {
      base = realpathSync(within);
      file = realpathSync(resolve(directory, name));
    } catch {
      return undefined;
    }
    if (!file.startsWith(base.endsWith(sep) ? base : base + sep)) {
      return null;
    }
    try {
      return readFileSync(file, "utf8");
    } catch {
      return undefined;
    }
  };
}

export async function composedFrom(path, host, hosting) {
  await load(await readFile(WASM));
  return Herald.compose(host, await readFile(path, "utf8"), beside(path), hosting);
}
