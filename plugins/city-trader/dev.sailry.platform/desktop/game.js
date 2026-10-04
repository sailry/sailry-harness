import { BID_INCREMENT, MAX_LEVEL, PASS_START, ROUND_LIMIT,
  SEATS, START_CASH } from "./data.js";

import { createMap, cityEvent, TILE_COUNT } from "./map.js";

const seats = () => Array.from({ length: SEATS }, (_, seat) => seat);
const live = (game, seat) => game.players[seat].enabled && !game.players[seat].bankrupt;
const tileAt = (game, tile) => game.board[tile];
const owned = (game, seat) => game.board.filter(tile => tile.kind === "property"
  && game.properties[tile.index].owner === seat);
const value = (game, tile) => tile.price + game.properties[tile.index].level * tile.upgrade;
const salePrice = (game, tile) => Math.floor(value(game, tile) / 2);
const nextSeat = (seat, predicate) => {
  for (let step = 1; step <= SEATS; step++) {
    const candidate = (seat + step) % SEATS;
    if (predicate(candidate)) return candidate;
  }
  return null;
};
const record = (game, key, seat, details = {}) =>
  game.history.push({ key, round: game.round, seat, ...details });

export function newGame(random = Math.random) {
  if (typeof random !== "function") throw new Error("Invalid random source");
  const value = random();
  if (!Number.isFinite(value) || value < 0 || value >= 1) throw new Error("Invalid random source");
  const seed = Math.floor(value * 0x100000000), board = createMap(seed);
  return { version: 1, seed, board, phase: "roll", active: 0, turn: 0,
    players: seats().map(() => ({ cash: START_CASH, position: 0, bankrupt: false, enabled: true })),
    properties: board.map(() => ({ owner: null, level: 0 })),
    round: 1, limit: ROUND_LIMIT, winners: [], reason: null, result: null,
    history: [], eventIndex: 0, lastRoll: null, moveSeq: 0, bank: 0,
    auction: null, trade: null, debt: null, traded: false };
}

export function netWorth(game, seat) {
  if (!Number.isInteger(seat) || seat < 0 || seat >= SEATS || !live(game, seat)) return 0;
  return game.players[seat].cash + owned(game, seat)
    .reduce((total, tile) => total + value(game, tile), 0);
}

export function rent(game, tileIndex) {
  const tile = tileAt(game, tileIndex);
  if (tile?.kind !== "property") return 0;
  const owner = game.properties[tileIndex].owner;
  if (owner === null || !live(game, owner)) return 0;
  const monopoly = game.board.filter(candidate => candidate.group === tile.group)
    .every(candidate => game.properties[candidate.index].owner === owner);
  return tile.baseRent * (monopoly ? 2 : 1) * (1 + game.properties[tileIndex].level);
}

function finish(game, reason) {
  const scores = seats().map(seat => netWorth(game, seat));
  const best = Math.max(...scores);
  game.phase = "over";
  game.turn = null;
  game.auction = null;
  game.trade = null;
  game.debt = null;
  game.reason = reason;
  game.winners = seats().filter(seat => live(game, seat) && scores[seat] === best);
  game.result = { reason, scores, winners: [...game.winners] };
  record(game, "finish", game.active, { reason, scores, winners: [...game.winners] });
}

function advance(game) {
  if (seats().filter(seat => live(game, seat)).length <= 1) return finish(game, "last_standing");
  const next = nextSeat(game.active, seat => live(game, seat));
  if (next <= game.active) game.round++;
  if (game.round > game.limit) return finish(game, "round_limit");
  game.active = next;
  game.turn = next;
  game.phase = "roll";
  game.traded = false;
}

function manage(game) {
  game.phase = "manage";
  game.turn = game.active;
  game.debt = null;
}

function credit(game, seat, amount) {
  game.players[seat].cash += amount;
  game.bank -= amount;
}

function pay(game, seat, amount, creditor, reason, tile = null) {
  if (game.players[seat].cash < amount) {
    game.phase = "debt";
    game.turn = seat;
    game.debt = { amount, creditor, reason, tile };
    record(game, "debt", seat, { amount, creditor, reason, tile });
    return false;
  }
  game.players[seat].cash -= amount;
  if (creditor === null) game.bank += amount;
  else game.players[creditor].cash += amount;
  return true;
}

function settleDebt(game) {
  const { amount, creditor, reason, tile } = game.debt;
  if (game.players[game.active].cash < amount) return;
  pay(game, game.active, amount, creditor, reason, tile);
  record(game, "debt_paid", game.active, { amount, creditor, reason, tile });
  manage(game);
}

function bankruptcy(game) {
  const seat = game.active;
  const { amount, creditor } = game.debt;
  const recovered = game.players[seat].cash;
  if (creditor === null) game.bank += recovered;
  else game.players[creditor].cash += recovered;
  game.players[seat].cash = 0;
  game.players[seat].bankrupt = true;
  for (const tile of owned(game, seat)) game.properties[tile.index] = { owner: null, level: 0 };
  game.debt = null;
  record(game, "bankrupt", seat, { amount, creditor, recovered });
  advance(game);
}

function land(game, tileIndex) {
  const tile = tileAt(game, tileIndex), seat = game.active;
  if (tile.kind === "property") {
    const owner = game.properties[tileIndex].owner;
    if (owner === null) { game.phase = "buy"; return; }
    if (owner !== seat) {
      const amount = rent(game, tileIndex);
      record(game, "rent", seat, { tile: tileIndex, owner, amount });
      if (!pay(game, seat, amount, owner, "rent", tileIndex)) return;
    }
  } else if (tile.kind === "bonus") {
    credit(game, seat, tile.amount);
    record(game, "bonus", seat, { tile: tileIndex, amount: tile.amount });
  } else if (tile.kind === "tax") {
    record(game, "tax", seat, { tile: tileIndex, amount: tile.amount });
    if (!pay(game, seat, tile.amount, null, "tax", tileIndex)) return;
  } else if (tile.kind === "event") {
    const event = cityEvent(game.seed, game.eventIndex);
    game.eventIndex++;
    record(game, "event", seat, { tile: tileIndex, event: event.key, amount: event.amount });
    if (event.amount > 0) credit(game, seat, event.amount);
    else if (!pay(game, seat, -event.amount, null, "event", tileIndex)) return;
  }
  manage(game);
}

function minimumBid(game) {
  const { tile, bid } = game.auction;
  return bid ? bid + BID_INCREMENT
    : Math.ceil(game.board[tile].price / 2 / BID_INCREMENT) * BID_INCREMENT;
}

function bidChoices(game) {
  const minimum = minimumBid(game), cash = game.players[game.turn].cash;
  return [...new Set([minimum, minimum + BID_INCREMENT, minimum + 3 * BID_INCREMENT])]
    .filter(amount => amount <= cash).map(amount => ({ kind: "bid", amount }));
}

function advanceAuction(game, seat) {
  const auction = game.auction;
  const canAct = candidate => live(game, candidate) && !auction.passed[candidate]
    && candidate !== auction.leader;
  const next = nextSeat(seat, canAct);
  if (next !== null) { game.turn = next; return; }
  if (auction.leader !== null) {
    const winner = auction.leader, amount = auction.bid;
    game.players[winner].cash -= amount;
    game.bank += amount;
    game.properties[auction.tile] = { owner: winner, level: 0 };
    record(game, "auction_won", winner, { tile: auction.tile, amount });
  } else record(game, "auction_unsold", game.active, { tile: auction.tile });
  game.auction = null;
  manage(game);
}

function startAuction(game) {
  const tile = game.players[game.active].position;
  game.auction = { tile, bid: 0, leader: null, passed: seats().map(() => false) };
  game.phase = "auction";
  game.turn = nextSeat(game.active, seat => live(game, seat));
  record(game, "auction_started", game.active, { tile });
}

function sameAction(left, right) {
  if (!right || typeof right !== "object" || Array.isArray(right)) return false;
  const keys = Object.keys(left);
  return Object.keys(right).length === keys.length
    && keys.every(key => Object.hasOwn(right, key) && right[key] === left[key]);
}

export function choices(game) {
  if (game.phase === "buy") {
    const tile = game.board[game.players[game.active].position];
    return [...(game.players[game.active].cash >= tile.price ? [{ kind: "buy" }] : []),
      { kind: "decline" }];
  }
  if (game.phase === "auction") return [{ kind: "pass" }, ...bidChoices(game)];
  if (game.phase === "trade") return [{ kind: "accept" }, { kind: "reject" }];
  if (game.phase === "debt") return [
    ...owned(game, game.active).map(tile => ({ kind: "sell", tile: tile.index })),
    { kind: "bankrupt" },
  ];
  if (!["roll", "manage"].includes(game.phase)) return [];
  const seat = game.active, cash = game.players[seat].cash;
  const options = [{ kind: game.phase === "roll" ? "roll" : "end" }];
  for (const tile of owned(game, seat)) {
    const monopoly = game.board.filter(candidate => candidate.group === tile.group)
      .every(candidate => game.properties[candidate.index].owner === seat);
    if (monopoly && game.properties[tile.index].level < MAX_LEVEL && cash >= tile.upgrade)
      options.push({ kind: "upgrade", tile: tile.index });
    options.push({ kind: "sell", tile: tile.index });
  }
  if (!game.traded) for (const tile of game.board.filter(candidate => candidate.kind === "property")) {
    const to = game.properties[tile.index].owner;
    if (to === null || to === seat || !live(game, to)) continue;
    for (const numerator of [100, 125, 150]) {
      const price = Math.ceil(value(game, tile) * numerator / 100 / 10) * 10;
      if (cash >= price) options.push({ kind: "propose", tile: tile.index, to, price });
    }
  }
  return options;
}

function die(random) {
  const value = random();
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value >= 1)
    throw new Error("Invalid die source");
  return Math.floor(value * 6) + 1;
}

export function act(game, action, random = Math.random) {
  if (!choices(game).some(candidate => sameAction(candidate, action))) return false;
  const seat = game.turn;
  switch (action.kind) {
    case "roll": {
      const dice = [die(random), die(random)];
      const from = game.players[seat].position, total = dice[0] + dice[1];
      const to = (from + total) % game.board.length, passed = from + total >= game.board.length;
      game.players[seat].position = to;
      game.moveSeq++;
      game.lastRoll = { seq: game.moveSeq, seat, from, to, dice, passed };
      record(game, "roll", seat, { from, to, dice, total });
      if (passed) {
        credit(game, seat, PASS_START);
        record(game, "passed_start", seat, { amount: PASS_START });
      }
      land(game, to);
      break;
    }
    case "buy": {
      const tile = game.players[seat].position;
      const amount = game.board[tile].price;
      game.players[seat].cash -= amount;
      game.bank += amount;
      game.properties[tile] = { owner: seat, level: 0 };
      record(game, "buy", seat, { tile, amount });
      manage(game);
      break;
    }
    case "decline":
      record(game, "decline", seat, { tile: game.players[seat].position });
      startAuction(game);
      break;
    case "bid":
      game.auction.bid = action.amount;
      game.auction.leader = seat;
      record(game, "bid", seat, { tile: game.auction.tile, amount: action.amount });
      advanceAuction(game, seat);
      break;
    case "pass":
      game.auction.passed[seat] = true;
      record(game, "pass", seat, { tile: game.auction.tile });
      advanceAuction(game, seat);
      break;
    case "end":
      record(game, "end", seat);
      advance(game);
      break;
    case "upgrade": {
      const tile = game.board[action.tile];
      game.players[seat].cash -= tile.upgrade;
      game.bank += tile.upgrade;
      game.properties[action.tile].level++;
      record(game, "upgrade", seat, { tile: action.tile,
        level: game.properties[action.tile].level, amount: tile.upgrade });
      break;
    }
    case "sell": {
      const tile = game.board[action.tile], amount = salePrice(game, tile);
      credit(game, seat, amount);
      game.properties[action.tile] = { owner: null, level: 0 };
      record(game, "sell", seat, { tile: action.tile, amount });
      if (game.phase === "debt") settleDebt(game);
      break;
    }
    case "propose":
      game.trade = { tile: action.tile, to: action.to, price: action.price,
        ...(game.phase === "roll" ? { beforeRoll: true } : {}) };
      game.traded = true;
      game.phase = "trade";
      game.turn = action.to;
      record(game, "trade_proposed", seat, { ...game.trade });
      break;
    case "accept": {
      const { tile, to, price, beforeRoll } = game.trade;
      game.players[game.active].cash -= price;
      game.players[to].cash += price;
      game.properties[tile].owner = game.active;
      record(game, "trade_accepted", to, { buyer: game.active, tile, price });
      game.trade = null;
      manage(game);
      if (beforeRoll) game.phase = "roll";
      break;
    }
    case "reject": {
      const beforeRoll = game.trade.beforeRoll;
      record(game, "trade_rejected", seat, { buyer: game.active, ...game.trade });
      game.trade = null;
      manage(game);
      if (beforeRoll) game.phase = "roll";
      break;
    }
    case "bankrupt":
      bankruptcy(game);
      break;
  }
  return true;
}

export function observation(game) {
  return { phase: game.phase, round: game.round, limit: game.limit, active: game.active,
    turn: game.turn, players: game.players.map(player => ({ ...player })),
    board: game.board,
    properties: game.properties.map(property => ({ ...property })),
    rents: game.board.map(tile => rent(game, tile.index)),
    worth: seats().map(seat => netWorth(game, seat)),
    tile: game.players[game.active].position, auction: game.auction,
    trade: game.trade, debt: game.debt, nextEvent: cityEvent(game.seed, game.eventIndex),
    recent: game.history.slice(-8) };
}

export function turn(game, moves) {
  return { instructions: "Play Dream City Tycoon, a property game for up to three players. You are the acting player. "
    + "Board indices run 0–23 along the generated closed route. Disabled players do not act. Passing start pays 200. Buy property, collect rent, improve complete "
    + "color groups, bid in auctions and offer cash to buy rivals' property. A full group doubles "
    + "base rent; each "
    + "upgrade adds another rent multiple. Bank sales return half of property plus upgrade cost. "
    + "Cash cannot go below zero; debt requires selling or bankruptcy. The game ends after 20 rounds "
    + "by net worth or when one player remains. "
    + "Choose one offered legal action by its id.",
    state: observation(game), choices: moves.map((move, id) => ({ id, ...move })) };
}

export function restore(value) {
  const invalid = () => { throw new Error("Invalid saved game"); };
  const integer = (value, min, max) => Number.isSafeInteger(value) && value >= min && value <= max;
  if (!value || value.version !== 1 || !integer(value.seed, 0, 0xffffffff)
    || !Array.isArray(value.board) || value.board.length !== TILE_COUNT) return invalid();
  const board = createMap(value.seed);
  if (value.board.some((tile, index) => !tile || Object.keys(tile).length !== Object.keys(board[index]).length
    || Object.keys(board[index]).some(key => tile[key] !== board[index][key]))) return invalid();
  if (!["roll", "buy", "auction", "manage", "trade", "debt", "over"].includes(value.phase)
    || !integer(value.active, 0, 2) || !integer(value.round, 1, ROUND_LIMIT + 1)
    || value.limit !== ROUND_LIMIT || !Array.isArray(value.players) || value.players.length !== SEATS
    || !Array.isArray(value.properties) || value.properties.length !== board.length
    || !Array.isArray(value.history) || !Array.isArray(value.winners)
    || !integer(value.eventIndex, 0, 10000) || !integer(value.moveSeq, 0, 10000)
    || typeof value.traded !== "boolean" || !Number.isSafeInteger(value.bank)) return invalid();
  if (value.players.some(player => !player || !integer(player.cash, 0, 10000000)
    || !integer(player.position, 0, TILE_COUNT - 1) || typeof player.bankrupt !== "boolean"
    || typeof player.enabled !== "boolean") || !value.players[0].enabled
    || value.players.filter(player => player.enabled).length < 2) return invalid();
  if (value.properties.some((property, tile) => !property
    || !(property.owner === null || integer(property.owner, 0, 2))
    || !integer(property.level, 0, MAX_LEVEL)
    || (property.owner === null && property.level !== 0)
    || (board[tile].kind !== "property" && property.owner !== null))) return invalid();
  if (value.phase !== "over" && (value.turn !== 0 || value.players[0].bankrupt)) return invalid();
  if (value.phase === "over" && (!value.result || !Array.isArray(value.result.scores)
    || value.result.scores.length !== SEATS || value.turn !== null)) return invalid();
  if (value.phase === "auction" && (!value.auction || !integer(value.auction.tile, 0, TILE_COUNT - 1)
    || !integer(value.auction.bid, 0, 10000000) || !Array.isArray(value.auction.passed)
    || value.auction.passed.length !== SEATS)) return invalid();
  if (value.phase === "trade" && (!value.trade || value.trade.to !== 0
    || !integer(value.trade.tile, 0, TILE_COUNT - 1) || !integer(value.trade.price, 1, 10000000))) return invalid();
  if (value.phase === "debt" && (!value.debt || !integer(value.debt.amount, 1, 10000000))) return invalid();
  if (value.history.some(entry => !entry || typeof entry.key !== "string" || !integer(entry.seat, 0, 2))
    || value.winners.some(seat => !integer(seat, 0, 2))) return invalid();
  if (value.lastRoll && (!integer(value.lastRoll.seat, 0, 2)
    || !integer(value.lastRoll.from, 0, TILE_COUNT - 1) || !integer(value.lastRoll.to, 0, TILE_COUNT - 1)
    || !integer(value.lastRoll.seq, 1, value.moveSeq) || !Array.isArray(value.lastRoll.dice)
    || value.lastRoll.dice.length !== 2 || value.lastRoll.dice.some(die => !integer(die, 1, 6)))) return invalid();
  if (value.phase === "over" && (value.result.scores.some(score => !integer(score, 0, 10000000))
    || !value.winners.length)) return invalid();
  if (value.phase === "auction" && (board[value.auction.tile].kind !== "property"
    || value.auction.passed.some(passed => typeof passed !== "boolean")
    || !(value.auction.leader === null || integer(value.auction.leader, 0, 2)))) return invalid();
  if (value.phase === "trade" && (board[value.trade.tile].kind !== "property"
    || value.properties[value.trade.tile].owner !== 0 || value.active === 0)) return invalid();
  if (value.phase === "debt" && !(value.debt.creditor === null
    || integer(value.debt.creditor, 0, 2))) return invalid();
  const game = JSON.parse(JSON.stringify(value));
  if (game.phase !== "over" && !choices(game).length) return invalid();
  return game;
}
