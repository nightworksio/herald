import { rounds } from "../../host.mjs";
import { composed } from "./herald.mjs";

export default async () => {
  await rounds(await composed(), ["poll"]);
};

export const config = { schedule: "*/5 * * * *" };
