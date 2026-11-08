import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

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
