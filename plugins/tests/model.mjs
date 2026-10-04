// Test double for the native sailry/sdk model boundary. Production parsing is tested in Rust.
export const retryable = ["failed", "timedOut", "invalidResponse", "unconfirmed"];

export function command(player, turn) {
  if (player?.kind !== "provider") throw new Error("configure");
  const { instructions, state, choices } = turn;
  return { kind: "generate_plugin_text", data: { model: player.model, effort: player.effort,
    prompt: instructions + ' Return ONLY {"move":id} with no explanation.\n'
      + JSON.stringify({ state, choices }) } };
}

export function select(result, player, moves) {
  if (player?.kind !== "provider") throw new Error("configure");
  if (result.Err) throw new Error(failure(result.Err));
  if (result.Ok?.kind !== "plugin_text") {
    throw new Error("invalidResponse");
  }
  let id;
  const reply = result.Ok.data?.text;
  if (typeof reply !== "string") throw new Error("invalidResponse");
  try {
    id = JSON.parse(reply.trim().replace(/^\`\`\`(?:json)?\s*/, "")
      .replace(/\s*\`\`\`$/, "")).move;
  } catch (_) { throw new Error("invalidResponse"); }
  if (!Number.isSafeInteger(id) || id < 0 || id >= moves.length) throw new Error("invalidResponse");
  return { move: moves[id], tokens: result.Ok.data.tokens || 0 };
}

export function failure(fault) {
  if (fault?.code === "not_configured") return "configure";
  if (fault?.message === "plugin model request timed out") return "timedOut";
  return "failed";
}

export async function completion(id, execute, outcome, sleep) {
  const result = JSON.parse(await execute(id));
  if (!result.Err) return result;
  while (true) {
    const receipt = JSON.parse(await outcome(id));
    if (receipt.Ok?.kind === "completed") return receipt.Ok.data;
    if (receipt.Ok?.kind !== "admitted") break;
    await sleep(1000);
  }
  throw new Error("unconfirmed");
}

export function errorCode(message) {
  const code = message?.replace(/^`sailry\/sdk\.[A-Za-z]+`: /, "");
  return ["configure", "failed", "invalidResponse", "unconfirmed", "timedOut"].includes(code) ? code : "failed";
}
