export const PLAYERS = 3;
export const STARTING_DICE = 5;

export function active(counts) {
  return counts.map((count, player) => count > 0 ? player : null).filter(player => player !== null);
}

export function nextActive(counts, player) {
  for (let step = 1; step <= PLAYERS; step++) {
    const next = (player + step) % PLAYERS;
    if (counts[next] > 0) return next;
  }
  return null;
}

export function deal(random = Math.random, counts = [5, 5, 5], starter = 0, round = 1) {
  if (counts.length !== PLAYERS || counts.some(count => !Number.isInteger(count) || count < 0 || count > STARTING_DICE)
    || active(counts).length < 2 || !Number.isInteger(starter) || starter < 0
    || starter >= PLAYERS || counts[starter] === 0) {
    throw new Error("Invalid dice match");
  }
  const hands = counts.map(count => Array.from({ length: count }, () => 1 + Math.floor(random() * 6)));
  return { phase: "bid", round, counts: [...counts], hands, starter, turn: starter,
    currentBid: null, history: [], result: null };
}

export function legalBid(game, quantity, face) {
  const current = game.currentBid;
  return game.phase === "bid" && Number.isInteger(quantity) && Number.isInteger(face)
    && quantity >= 1 && quantity <= game.counts.reduce((sum, count) => sum + count, 0)
    && face >= 1 && face <= 6
    && (!current || quantity > current.quantity || quantity === current.quantity && face > current.face);
}

export function bid(game, player, quantity, face) {
  if (game.turn !== player || !legalBid(game, quantity, face)) return false;
  game.currentBid = { player, quantity, face };
  game.history.push({ player, kind: "bid", quantity, face });
  game.turn = nextActive(game.counts, player);
  return true;
}

export function challenge(game, player) {
  if (game.phase !== "bid" || game.turn !== player || !game.currentBid) return false;
  const { player: bidder, quantity, face } = game.currentBid;
  const actual = game.hands.flat().filter(die => die === face).length;
  const loser = actual >= quantity ? player : bidder;
  game.counts[loser]--;
  game.history.push({ player, kind: "challenge" });
  const remaining = active(game.counts);
  game.phase = remaining.length === 1 ? "match_over" : "round_over";
  game.result = { bidder, challenger: player, quantity, face, actual, loser,
    nextStarter: game.counts[loser] > 0 ? loser : nextActive(game.counts, loser),
    winner: remaining.length === 1 ? remaining[0] : null };
  game.turn = null;
  return true;
}

export function nextRound(game, random = Math.random) {
  if (game.phase !== "round_over") return null;
  return deal(random, game.counts, game.result.nextStarter, game.round + 1);
}

export function choices(game) {
  if (game.phase !== "bid") return [];
  const moves = game.currentBid ? [{ kind: "challenge" }] : [];
  const total = game.counts.reduce((sum, count) => sum + count, 0);
  for (let quantity = 1; quantity <= total; quantity++) {
    for (let face = 1; face <= 6; face++) {
      if (legalBid(game, quantity, face)) moves.push({ kind: "bid", quantity, face });
    }
  }
  return moves;
}

// Only the acting player's concealed dice cross the model boundary.
export function observation(game) {
  return { round: game.round, player: game.turn, hand: [...game.hands[game.turn]],
    counts: [...game.counts], currentBid: game.currentBid && { ...game.currentBid },
    history: game.history.map(entry => ({ ...entry })) };
}

export function turn(game, moves) {
  return { instructions: "Play Liar's Dice. Each enabled player starts with five dice; "
    + "players with zero dice are out. There are no wild faces. A bid is a quantity and face, "
    + "increasing first by quantity then by face. A challenge reveals every die: if the exact "
    + "face count meets the bid, the challenger loses one die; otherwise the bidder loses one. "
    + "Only your dice and public bids are visible. Every offered move is legal. Pick one ID.",
  state: observation(game), choices: moves.map((move, id) => ({ id, ...move })) };
}
