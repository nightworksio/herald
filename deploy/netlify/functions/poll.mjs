import { rounds } from "../../host.mjs";
import { composed } from "./herald.mjs";

export default async function poll() {
  await rounds(await composed(), ["poll"]);
}

export const config = { schedule: "*/5 * * * *" };
