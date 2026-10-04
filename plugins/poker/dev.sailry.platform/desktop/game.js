import { compare, evaluate, label } from "./cards.js";

export const SMALL_BLIND = 5;
export const BIG_BLIND = 10;
const streets = ["preflop", "flop", "turn", "river"];

function shuffle(random) {
  const deck = Array.from({ length: 52 }, (_, index) => index);
  for (let index = deck.length - 1; index > 0; index--) {
    const other = Math.floor(random() * (index + 1));
    [deck[index], deck[other]] = [deck[other], deck[index]];
  }
  return deck;
}

function put(game, player, amount) {
  game.stacks[player] -= amount;
  game.streetBets[player] += amount;
  game.contributions[player] += amount;
  game.pot += amount;
}

export function deal(random = Math.random, stacks = [1000, 1000], dealer = 0, handNumber = 1) {
  if (stacks.length !== 2 || stacks.some(value => !Number.isInteger(value) || value < 0)
    || stacks.some(value => value === 0) || ![0, 1].includes(dealer)) throw new Error("Invalid poker match");
  const deck = shuffle(random);
  const game = { phase: "betting", stage: "preflop", handNumber, dealer, turn: dealer,
    hands: [[deck[0], deck[2]], [deck[1], deck[3]]], board: deck.slice(4, 9), shown: 0,
    stacks: [...stacks], startStacks: [...stacks], streetBets: [0, 0], contributions: [0, 0],
    currentBet: 0, minRaise: BIG_BLIND, acted: [false, false], pot: 0, history: [], settlement: null };
  put(game, dealer, Math.min(SMALL_BLIND, game.stacks[dealer]));
  put(game, 1 - dealer, Math.min(BIG_BLIND, game.stacks[1 - dealer]));
  game.currentBet = Math.max(...game.streetBets);
  resolve(game);
  return game;
}

export function visibleBoard(game) { return game.board.slice(0, game.shown); }

export function legal(game, player = game.turn) {
  if (game.phase !== "betting" || game.turn !== player || game.stacks[player] === 0) return null;
  const other = 1 - player;
  const toCall = Math.max(0, game.currentBet - game.streetBets[player]);
  const maxTo = game.streetBets[player] + game.stacks[player];
  const minTo = game.currentBet + game.minRaise;
  return { toCall, callAmount: Math.min(toCall, game.stacks[player]), minTo, maxTo,
    canRaise: maxTo > game.currentBet && game.stacks[other] > 0,
    canShortRaise: maxTo > game.currentBet && maxTo < minTo };
}

function refund(game) {
  const excess = game.contributions[0] - game.contributions[1];
  if (excess !== 0) {
    const player = excess > 0 ? 0 : 1;
    game.stacks[player] += Math.abs(excess);
    game.contributions[player] -= Math.abs(excess);
    game.pot -= Math.abs(excess);
  }
}

function settle(game, winner, reason) {
  if (reason === "showdown") refund(game);
  const pot = game.pot;
  if (winner === null) {
    game.stacks[0] += Math.floor(pot / 2);
    game.stacks[1] += Math.floor(pot / 2);
    game.stacks[game.dealer] += pot % 2;
  } else game.stacks[winner] += pot;
  game.pot = 0;
  game.phase = "over";
  game.turn = null;
  const values = reason === "showdown" ? game.hands.map(hand => evaluate([...hand, ...game.board])) : null;
  game.settlement = { winner, reason, pot, values, net: game.stacks[0] - game.startStacks[0],
    matchOver: game.stacks.some(stack => stack === 0) };
}

function showdown(game) {
  game.shown = 5;
  const values = game.hands.map(hand => evaluate([...hand, ...game.board]));
  const difference = compare(values[0], values[1]);
  settle(game, difference === 0 ? null : difference > 0 ? 0 : 1, "showdown");
}

function nextStreet(game) {
  const index = streets.indexOf(game.stage);
  if (index === streets.length - 1) return showdown(game);
  game.stage = streets[index + 1];
  game.shown = [0, 3, 4, 5][index + 1];
  game.streetBets = [0, 0];
  game.currentBet = 0;
  game.minRaise = BIG_BLIND;
  game.acted = [false, false];
  game.turn = 1 - game.dealer;
}

function resolve(game) {
  if (game.phase !== "betting") return;
  // With one stack empty, the other player may still owe a call. Once matched,
  // no further bet can be contested and all remaining community cards run out.
  if (game.stacks.some(stack => stack === 0)) {
    const live = game.stacks[0] > 0 ? 0 : game.stacks[1] > 0 ? 1 : null;
    if (live !== null && game.currentBet > game.streetBets[live]) { game.turn = live; return; }
    return showdown(game);
  }
  if (game.acted.every(Boolean) && game.streetBets[0] === game.streetBets[1]) nextStreet(game);
}

export function act(game, player, move) {
  const options = legal(game, player);
  if (!options || !move || typeof move !== "object") return false;
  const other = 1 - player;
  let amount = 0;
  if (move.kind === "fold") {
    if (!options.toCall) return false;
    game.history.push({ stage: game.stage, player, kind: "fold", amount: 0 });
    settle(game, other, "fold");
    return true;
  }
  if (move.kind === "check") {
    if (options.toCall) return false;
  } else if (move.kind === "call") {
    if (!options.toCall) return false;
    amount = options.callAmount;
  } else if (move.kind === "raise") {
    const target = move.to;
    if (!options.canRaise || !Number.isInteger(target) || target > options.maxTo
      || target <= game.currentBet || (target < options.minTo
        && !(target === options.maxTo && options.canShortRaise))) return false;
    amount = target - game.streetBets[player];
    const increase = target - game.currentBet;
    if (increase >= game.minRaise) game.minRaise = increase;
    game.currentBet = target;
    game.acted[other] = false;
  } else return false;
  if (amount) put(game, player, amount);
  game.acted[player] = true;
  game.history.push({ stage: game.stage, player, kind: move.kind, amount,
    ...(move.kind === "raise" ? { to: game.currentBet } : {}) });
  game.turn = other;
  resolve(game);
  return true;
}

export function choices(game) {
  const options = legal(game);
  if (!options) return [];
  const moves = [options.toCall ? { kind: "call" } : { kind: "check" }];
  if (options.toCall) moves.push({ kind: "fold" });
  if (options.canRaise) {
    const calledPot = game.pot + options.toCall;
    const targets = [options.minTo,
      game.currentBet + Math.max(game.minRaise, Math.ceil(calledPot / 2)),
      game.currentBet + Math.max(game.minRaise, calledPot), options.maxTo];
    for (const target of targets) {
      const to = Math.min(target, options.maxTo);
      if (to >= options.minTo || (to === options.maxTo && options.canShortRaise)) {
        if (!moves.some(move => move.to === to)) moves.push({ kind: "raise", to });
      }
    }
  }
  return moves;
}

export function observation(game) {
  const player = game.turn;
  return { stage: game.stage, handNumber: game.handNumber, player, dealer: game.dealer,
    hand: game.hands[player].map(label), board: visibleBoard(game).map(label),
    stacks: game.stacks, pot: game.pot, streetBets: game.streetBets,
    history: game.history.map(entry => ({ ...entry })) };
}

export function turn(game, moves) {
  return { instructions: "Play heads-up no-limit Texas Hold'em. You are player 1. "
    + "The other hand and future board are hidden. Blinds are 5/10. "
    + "Evaluate your hand, board, pot odds, stack sizes and previous actions. "
    + "Every offered move is legal; choose one ID.",
  state: observation(game), choices: moves.map((move, id) => ({ id, ...move })) };
}
