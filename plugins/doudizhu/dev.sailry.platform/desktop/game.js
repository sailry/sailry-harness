import { rank, sort, pattern, beats, legal } from "./cards.js";

export function deal(random = Math.random, first = 0) {
  const deck = Array.from({ length: 54 }, (_, index) => index);
  for (let index = deck.length - 1; index > 0; index--) {
    const other = Math.floor(random() * (index + 1));
    [deck[index], deck[other]] = [deck[other], deck[index]];
  }
  return { phase: "bid", hands: [0, 1, 2].map(index => sort(deck.slice(index * 17, index * 17 + 17))),
    bottom: deck.slice(51), turn: first, landlord: null, bid: 0, bids: [], last: null,
    passes: 0, history: [], played: [0, 0, 0], multiplier: 1, winner: null };
}

export function bid(game, player, amount) {
  if (game.phase !== "bid" || game.turn !== player || !Number.isInteger(amount)
    || amount < 0 || amount > 3 || (amount && amount <= game.bid)) return false;
  game.bids.push({ player, amount });
  if (amount) { game.bid = amount; game.landlord = player; }
  if (amount === 3 || game.bids.length === 3) {
    if (game.landlord === null) game.phase = "redeal";
    else {
      game.phase = "play";
      game.hands[game.landlord] = sort([...game.hands[game.landlord], ...game.bottom]);
      game.turn = game.landlord;
    }
  } else game.turn = (player + 1) % 3;
  return true;
}

export function play(game, player, cards) {
  if (game.phase !== "play" || game.turn !== player) return false;
  const shape = pattern(cards);
  if (!cards.length) {
    if (!game.last || game.last.player === player) return false;
    game.passes++;
    game.history.push({ player, cards: [] });
    if (game.passes === 2) { game.last = null; game.passes = 0; }
  } else {
    if (!cards.every(card => game.hands[player].includes(card))
      || !beats(shape, game.last?.shape)) return false;
    game.hands[player] = game.hands[player].filter(card => !cards.includes(card));
    game.last = { player, cards: sort(cards), shape };
    game.passes = 0;
    game.played[player]++;
    game.history.push({ player, cards: sort(cards) });
    if (["bomb", "rocket"].includes(shape.kind)) game.multiplier *= 2;
    if (!game.hands[player].length) {
      game.phase = "over";
      game.winner = player;
      if (player === game.landlord ? game.played.every((count, seat) => seat === player || count === 0)
        : game.played[game.landlord] === 1) game.multiplier *= 2;
    }
  }
  game.turn = (player + 1) % 3;
  return true;
}

export function tablePlay(game) {
  return game.history.findLast(entry => entry.cards.length > 0) || null;
}

export function choices(game) {
  if (game.phase === "bid") return [0, 1, 2, 3].filter(value => !value || value > game.bid);
  if (game.phase !== "play") return [];
  const candidates = legal(game.hands[game.turn], game.last?.shape);
  // Avoid asking a reasoning model to search dozens of equivalent wing variants.
  // Keep both ends of each card type; legal() orders by cards shed, then rank.
  const types = new Map();
  for (const move of candidates) {
    const kind = pattern(move).kind;
    if (!types.has(kind)) types.set(kind, [move, move]);
    else types.get(kind)[1] = move;
  }
  const moves = [...types.values()].flatMap(([first, last]) => first === last ? [first] : [first, last]);
  if (game.last && game.last.player !== game.turn) moves.push([]);
  return moves;
}

// Only the acting player's private hand crosses the model boundary.
export function observation(game) {
  return { phase: game.phase, player: game.turn, landlord: game.landlord, bid: game.bid,
    bids: game.bids, hand: game.hands[game.turn].map(rank),
    counts: game.hands.map(hand => hand.length),
    bottom: game.phase === "bid" ? [] : game.bottom.map(rank),
    last: game.last && { player: game.last.player, cards: game.last.cards.map(rank) },
    history: game.history.map(entry => ({ player: entry.player, cards: entry.cards.map(rank) })) };
}

export function turn(game, moves) {
  return { instructions: "Play three-player Dou Dizhu. You are the acting player. Landlord plays "
    + "against both farmers; farmers cooperate. Cards use numeric ranks 3..17: "
    + "11=J,12=Q,13=K,14=A,15=2,16=small joker,17=big joker. Suits do not matter. "
    + "Every choice is already legal and has an explicit id and card type. Do not recheck "
    + "legality or count array positions. Pick a good move from the current hand using a quick "
    + "heuristic; do not simulate future deals or search complete game trees.",
  state: observation(game), choices: moves.map((move, id) =>
    game.phase === "bid" ? { id, bid: move } : { id, kind: pattern(move)?.kind || "pass", cards: move.map(rank) }) };
}
