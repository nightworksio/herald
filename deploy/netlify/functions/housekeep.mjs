import { rounds } from "../../host.mjs";
import { composed } from "./herald.mjs";

export default async function housekeep() {
  await rounds(await composed(), ["housekeep"]);
}

export const config = { schedule: "17 * * * *" };
