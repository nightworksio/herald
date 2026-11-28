import { readFileSync, realpathSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { delimiter, dirname, isAbsolute, resolve, sep } from "node:path";
import { Herald, load } from "./host.mjs";

const WASM = new URL("./pkg/herald_wasm_bg.wasm", import.meta.url);

export const FILES_ALLOWED = "HERALD_FILES_ALLOWED";

function resolved(path) {
  try {
    const real = realpathSync(path);
    return statSync(real).isDirectory() ? real : undefined;
  } catch {
    return undefined;
  }
}

export function beside(config, listed) {
  const directory = dirname(config);
  const read = (name, within = directory) => {
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
  read.resolved = resolved;
  if (listed) {
    read.allowed = listed.split(delimiter).map((entry) => {
      const real = isAbsolute(entry) ? resolved(entry) : undefined;
      if (real === undefined) {
        throw new Error(`${FILES_ALLOWED} lists a directory that is not absolute or not a directory on this host; list each directory from the root, joined as PATH joins its own`);
      }
      return real;
    });
  } else {
    read.beside = resolved(directory);
  }
  return read;
}

export async function composedFrom(path, host, hosting, listed) {
  await load(await readFile(WASM));
  return Herald.compose(host, await readFile(path, "utf8"), beside(path, listed), hosting);
}
