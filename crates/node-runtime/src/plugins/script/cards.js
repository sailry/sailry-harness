// Draft ownership is added by Node after the admitted handler returns.
import { context, modelCall } from "sailry/sdk";
export function card(key, props, fallback) {
  return {kind:"card", key, props, fallback};
}
export async function status(server) {
  const scope = context();
  const output = modelCall("read", {kind:"read_mcp_authorization", data:{package:scope.package,server}});
  return {configured:output.data.configured === true};
}
export async function connectionStatus(card) {
  const scope = context();
  const output = modelCall("read", {kind:"read_plugin_connection", data:{package:scope.package,card}});
  return output.data;
}
