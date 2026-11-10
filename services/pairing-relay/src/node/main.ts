import { start } from "./server";

const path = process.env.SAILRY_RELAY_DATABASE;
const port = Number(process.env.SAILRY_RELAY_PORT ?? "8794");
if (!path || !Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("Set SAILRY_RELAY_DATABASE and a valid SAILRY_RELAY_PORT");
}
process.umask(0o077);
const service = await start(path, port);
console.log("Pairing service ready");
let stopping = false;
for (const signal of ["SIGTERM", "SIGINT"] as const) {
  process.on(signal, () => {
    if (stopping) return;
    stopping = true;
    void service.close().then(() => process.exit(0), () => process.exit(1));
  });
}
