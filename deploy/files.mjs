import { closeSync, fstatSync, openSync, readFileSync, realpathSync, statSync } from "node:fs";
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

function inside(base, file) {
  return file === base || file.startsWith(base.endsWith(sep) ? base : base + sep);
}

function opened(file) {
  const fd = openSync(file, "r");
  try {
    const held = fstatSync(fd);
    const now = statSync(file);
    if (realpathSync(file) !== file || held.dev !== now.dev || held.ino !== now.ino) {
      return null;
    }
    return readFileSync(fd, "utf8");
  } finally {
    closeSync(fd);
  }
}

export function beside(config, listed) {
  const directory = dirname(config);
  const own = resolved(directory);
  const read = (name, within) => {
    let file;
    try {
      file = realpathSync(resolve(directory, name));
    } catch {
      return undefined;
    }
    const bound = read.allowed ?? (own === undefined ? [] : [own]);
    const base = within ?? own;
    if (base === undefined || !inside(base, file) || (within !== undefined && !bound.some((each) => inside(each, file)))) {
      return null;
    }
    try {
      return opened(file);
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
    read.beside = own;
  }
  return read;
}

export async function composedFrom(path, host, hosting, listed) {
  await load(await readFile(WASM));
  return Herald.compose(host, await readFile(path, "utf8"), beside(path, listed), hosting);
}
