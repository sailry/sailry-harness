import test from "node:test";
import assert from "node:assert/strict";
import { act, choices, netWorth, newGame as createGame, observation, rent, restore, turn } from "../dev.sailry.platform/desktop/game.js";
import { BID_INCREMENT, EVENTS, PASS_START, ROUND_LIMIT, START_CASH } from "../dev.sailry.platform/desktop/data.js";

import { cityEvent } from "../dev.sailry.platform/desktop/map.js";
const newGame = () => createGame(() => 0);
const BOARD = newGame().board;

const dice = (...values) => {
  let index = 0;
  return () => values[index++];
};
const cash = game => game.players.reduce((total, player) => total + player.cash, 0);
const conserved = game => assert.equal(cash(game) + game.bank, 3 * START_CASH);
const choose = (game, kind, predicate = () => true) =>
  choices(game).find(action => action.kind === kind && predicate(action));
const rollTwo = game => act(game, { kind: "roll" }, dice(0, 0));

test("disabled seats skip auctions and turns and survive save restoration", () => {
  const game = newGame();
  game.players[1].enabled = false;
  assert(rollTwo(game));
  assert(act(game, { kind: "decline" }));
  assert.equal(game.turn, 2);
  assert(act(game, { kind: "pass" }));
  assert.equal(game.turn, 0);
  assert(act(game, { kind: "pass" }));
  const saved = restore(JSON.parse(JSON.stringify(game)));
  assert(act(saved, { kind: "end" }));
  assert.equal(saved.active, 2);
  assert.equal(saved.players[1].enabled, false);
  assert.equal(netWorth(saved, 1), 0);
  saved.phase = "manage";
  saved.round = saved.limit;
  assert(act(saved, { kind: "end" }));
  assert.equal(saved.phase, "over");
  assert.deepEqual(saved.winners, [0, 2]);
});

test("board retains the complete property economy and original special tiles", () => {
  assert.equal(BOARD.length, 24);
  assert.deepEqual(BOARD.filter(tile => tile.kind === "property").map(tile => tile.price).sort((a, b) => a - b),
    Array.from({ length: 18 }, (_, index) => 120 + index * 20));
  assert.equal(BOARD[0].kind, "start");
  for (const [kind, count] of [["event", 2], ["tax", 2], ["bonus", 1]])
    assert.equal(BOARD.filter(tile => tile.kind === kind).length, count);
  for (let group = 0; group < 6; group++)
    assert.equal(BOARD.filter(tile => tile.group === group).length, 3);
  assert.deepEqual(EVENTS.map(event => event.amount), [120, -80, 150, -100]);
});

test("rolling offers purchase and keeps bank transfers balanced", () => {
  const game = newGame();
  assert.deepEqual(choices(game), [{ kind: "roll" }]);
  assert(rollTwo(game));
  assert.deepEqual(game.lastRoll, { seq: 1, seat: 0, from: 0, to: 2,
    dice: [1, 1], passed: false });
  assert.equal(game.phase, "buy");
  assert.deepEqual(choices(game), [{ kind: "buy" }, { kind: "decline" }]);
  assert(act(game, { kind: "buy" }));
  assert.equal(game.properties[2].owner, 0);
  assert.equal(game.players[0].cash, START_CASH - BOARD[2].price);
  assert.equal(game.phase, "manage");
  conserved(game);
  assert(act(game, { kind: "end" }));
  assert.equal(game.phase, "roll");
  assert.equal(game.active, 1);
  assert.equal(game.round, 1);
});

test("property management remains available before rolling", () => {
  const game = newGame();
  const group = game.board.filter(tile => tile.kind === "property" && tile.group === 0);
  for (const tile of group) game.properties[tile.index].owner = 0;
  const property = group[0];
  assert(act(game, { kind: "upgrade", tile: property.index }));
  assert.equal(game.phase, "roll");
  assert.equal(game.properties[property.index].level, 1);
  assert(act(game, { kind: "sell", tile: property.index }));
  assert.equal(game.phase, "roll");
  assert.equal(game.properties[property.index].owner, null);
  assert(rollTwo(game));
  conserved(game);
});

test("pre-roll offers return to the buyer's roll after either response", () => {
  for (const response of ["accept", "reject"]) {
    const game = newGame();
    game.properties[1].owner = 1;
    assert(act(game, choose(game, "propose", move => move.tile === 1)));
    assert.equal(game.trade.beforeRoll, true);
    assert(act(game, { kind: response }));
    assert.equal(game.phase, "roll");
    assert.equal(game.turn, 0);
    assert.equal(game.properties[1].owner, response === "accept" ? 0 : 1);
    assert(!choices(game).some(move => move.kind === "propose"));
    assert(rollTwo(game));
    conserved(game);
  }
});

test("invalid actions and a broken die leave game unchanged", () => {
  const game = newGame();
  const before = structuredClone(game);
  for (const action of [null, { kind: "buy" }, { kind: "roll", extra: true },
    { kind: "roll", amount: 1 }, { kind: "end" }]) assert(!act(game, action));
  assert.deepEqual(game, before);
  assert.throws(() => act(game, { kind: "roll" }, dice(0, 1)), /Invalid die/);
  assert.deepEqual(game, before);
  assert.throws(() => createGame(null), /Invalid random/);
});

test("passing start, events, tax and bonus use a deterministic cash ledger", () => {
  const game = newGame();
  game.players[0].position = BOARD.length - 2;
  assert(rollTwo(game));
  assert.equal(game.players[0].position, 0);
  assert.equal(game.players[0].cash, START_CASH + PASS_START);
  assert.equal(game.history.at(-1).key, "passed_start");
  conserved(game);

  game.phase = "roll";
  game.players[0].position = 2;
  assert(rollTwo(game));
  assert.equal(game.players[0].position, 4);
  assert.equal(game.eventIndex, 1);
  assert.equal(game.players[0].cash, START_CASH + PASS_START + cityEvent(game.seed, 0).amount);
  conserved(game);

  game.phase = "roll";
  game.players[0].position = 6;
  assert(rollTwo(game));
  assert.equal(game.players[0].position, 8);
  assert.equal(game.players[0].cash, START_CASH + PASS_START + cityEvent(game.seed, 0).amount - 100);
  conserved(game);

  game.phase = "roll";
  game.players[0].position = 10;
  assert(rollTwo(game));
  assert.equal(game.players[0].position, 12);
  assert.equal(game.players[0].cash, START_CASH + PASS_START + cityEvent(game.seed, 0).amount - 100 + 150);
  conserved(game);
});

test("seeded event cards restore their draw order without creating negative cash", () => {
  const game = newGame();
  for (let index = 0; index < EVENTS.length; index++) {
    game.phase = "roll";
    game.players[0].position = 2;
    assert(rollTwo(game));
    assert.equal(game.eventIndex, index + 1);
    assert.equal(game.history.at(-1).event, cityEvent(game.seed, index).key);
    assert(game.players[0].cash >= 0);
    conserved(game);
  }
  game.phase = "roll";
  game.players[0].position = 2;
  assert(rollTwo(game));
  assert.equal(game.history.at(-1).event, cityEvent(game.seed, EVENTS.length).key);
});

test("rent, full-group improvement and bank sale use the listed values", () => {
  const game = newGame();
  game.phase = "manage";
  for (const tile of BOARD.filter(tile => tile.group === BOARD[2].group).map(tile => tile.index)) game.properties[tile].owner = 0;
  assert.equal(rent(game, 2), BOARD[2].baseRent * 2);
  assert(choose(game, "upgrade", action => action.tile === 2));
  assert(act(game, { kind: "upgrade", tile: 2 }));
  assert.equal(game.properties[2].level, 1);
  assert.equal(rent(game, 2), BOARD[2].baseRent * 4);
  const before = game.players[0].cash;
  assert(act(game, { kind: "sell", tile: 2 }));
  assert.equal(game.players[0].cash - before, (BOARD[2].price + BOARD[2].upgrade) / 2);
  assert.deepEqual(game.properties[2], { owner: null, level: 0 });
  const sibling = BOARD.find(tile => tile.group === BOARD[2].group && tile.index !== 2);
  assert.equal(rent(game, sibling.index), sibling.baseRent);
});

test("ascending auction sells to the last bidder at the final bid", () => {
  const game = newGame();
  assert(rollTwo(game));
  assert(act(game, { kind: "decline" }));
  assert.equal(game.phase, "auction");
  assert.equal(game.turn, 1);
  assert.deepEqual(game.auction, { tile: 2, bid: 0, leader: null,
    passed: [false, false, false] });
  assert(act(game, choose(game, "bid")));
  const opening = Math.ceil(BOARD[2].price / 2 / BID_INCREMENT) * BID_INCREMENT;
  assert.equal(game.auction.bid, opening);
  assert.equal(game.turn, 2);
  assert(act(game, { kind: "pass" }));
  assert.equal(game.turn, 0);
  assert(act(game, choose(game, "bid")));
  assert.equal(game.auction.bid, opening + BID_INCREMENT);
  assert.equal(game.turn, 1);
  assert(act(game, { kind: "pass" }));
  assert.equal(game.phase, "manage");
  assert.equal(game.properties[2].owner, 0);
  assert.equal(game.players[0].cash, START_CASH - opening - BID_INCREMENT);
  assert.equal(game.auction, null);
  conserved(game);
});

test("auction can end without a buyer and never exceeds a bidder's cash", () => {
  const game = newGame();
  game.players[1].cash = 0;
  assert(rollTwo(game));
  assert(act(game, { kind: "decline" }));
  assert.deepEqual(choices(game), [{ kind: "pass" }]);
  assert(act(game, { kind: "pass" }));
  assert(act(game, { kind: "pass" }));
  assert(act(game, { kind: "pass" }));
  assert.equal(game.phase, "manage");
  assert.equal(game.properties[2].owner, null);
  assert.equal(game.history.at(-1).key, "auction_unsold");
});

test("buyer offers cash to a rival who may accept or reject once per turn", () => {
  const game = newGame();
  game.phase = "manage";
  game.properties[2].owner = 1;
  const proposed = choose(game, "propose", action => action.tile === 2
    && action.to === 1 && action.price === BOARD[2].price);
  assert(proposed);
  assert(act(game, proposed));
  assert.equal(game.phase, "trade");
  assert.equal(game.turn, 1);
  assert.deepEqual(choices(game), [{ kind: "accept" }, { kind: "reject" }]);
  assert(act(game, { kind: "reject" }));
  assert.equal(game.phase, "manage");
  assert.equal(game.turn, 0);
  assert.equal(game.properties[2].owner, 1);
  assert(!choose(game, "propose"));

  game.traded = false;
  const offer = choose(game, "propose", action => action.tile === 2
    && action.to === 1 && action.price === BOARD[2].price);
  assert(act(game, offer));
  const before = game.players.map(player => player.cash);
  assert(act(game, { kind: "accept" }));
  assert.equal(game.properties[2].owner, 0);
  assert.equal(game.players[0].cash, before[0] - offer.price);
  assert.equal(game.players[1].cash, before[1] + offer.price);
  assert.equal(game.bank, 0);
  assert.equal(game.history.at(-1).key, "trade_accepted");
});

test("offers are limited by the buyer's current cash", () => {
  const game = newGame();
  game.phase = "manage";
  game.properties[1].owner = 1;
  game.players[0].cash = BOARD[1].price - 1;
  assert(!choose(game, "propose"));
  const before = structuredClone(game);
  assert(!act(game, { kind: "propose", tile: 1, to: 1, price: BOARD[1].price }));
  assert.deepEqual(game, before);
  game.players[0].cash++;
  assert.deepEqual(choices(game).filter(action => action.kind === "propose"),
    [{ kind: "propose", tile: 1, to: 1, price: BOARD[1].price }]);
});

test("rent moves cash between players and insufficient cash enters debt", () => {
  const game = newGame();
  game.properties[2].owner = 1;
  const owed = rent(game, 2);
  const before = game.players.map(player => player.cash);
  assert(rollTwo(game));
  assert.equal(game.phase, "manage");
  assert.equal(game.players[0].cash, before[0] - owed);
  assert.equal(game.players[1].cash, before[1] + owed);
  conserved(game);

  const another = newGame();
  another.properties[2].owner = 1;
  another.properties[23].owner = 0;
  another.players[0].cash = 10;
  assert(rollTwo(another));
  assert.equal(another.phase, "debt");
  assert.equal(another.debt.amount, owed);
  assert.deepEqual(choices(another), [{ kind: "sell", tile: 23 }, { kind: "bankrupt" }]);
  assert(act(another, { kind: "sell", tile: 23 }));
  assert.equal(another.phase, "manage");
  assert.equal(another.players[1].cash, START_CASH + owed);
  assert.equal(another.properties[23].owner, null);
  assert.equal(another.debt, null);
});

test("bankruptcy transfers remaining cash, returns property and skips the seat", () => {
  const game = newGame();
  game.properties[2].owner = 1;
  game.properties[23].owner = 0;
  game.players[0].cash = 10;
  assert(rollTwo(game));
  assert.equal(game.phase, "debt");
  assert(act(game, { kind: "bankrupt" }));
  assert.equal(game.players[0].cash, 0);
  assert(game.players[0].bankrupt);
  assert.equal(game.players[1].cash, START_CASH + 10);
  assert.deepEqual(game.properties[23], { owner: null, level: 0 });
  assert.equal(game.active, 1);
  assert.equal(game.turn, 1);
  assert.equal(game.phase, "roll");
});

test("the final solvent player wins immediately after another bankruptcy", () => {
  const game = newGame();
  game.players[2].bankrupt = true;
  game.players[0].cash = 10;
  game.properties[2].owner = 1;
  assert(rollTwo(game));
  assert.equal(game.phase, "debt");
  assert(act(game, { kind: "bankrupt" }));
  assert.equal(game.phase, "over");
  assert.equal(game.reason, "last_standing");
  assert.deepEqual(game.winners, [1]);
  assert.deepEqual(choices(game), []);
});

test("round limit ranks net worth and preserves tied winners", () => {
  const game = newGame();
  game.phase = "manage";
  game.active = 2;
  game.turn = 2;
  game.round = ROUND_LIMIT;
  game.players[0].cash = 1500;
  game.players[1].cash = 1500;
  game.players[2].cash = 900;
  assert.equal(netWorth(game, 0), 1500);
  assert(act(game, { kind: "end" }));
  assert.equal(game.phase, "over");
  assert.equal(game.reason, "round_limit");
  assert.deepEqual(game.winners, [0, 1]);
  assert.deepEqual(game.result.scores, [1500, 1500, 900]);
  assert.deepEqual(choices(game), []);
  assert(!act(game, { kind: "roll" }));
});

test("an entire deterministic match terminates with legal, balanced actions", () => {
  function match() {
    const game = newGame();
    let steps = 0;
    while (game.phase !== "over" && steps++ < 500) {
      const actions = choices(game);
      assert(actions.length > 0);
      const action = game.phase === "buy" ? (choose(game, "buy") || choose(game, "decline"))
        : game.phase === "debt" ? (choose(game, "sell") || choose(game, "bankrupt"))
          : actions[0];
      assert(action);
      assert(act(game, action, dice(0, 0)));
      assert(game.players.every(player => Number.isInteger(player.cash) && player.cash >= 0));
      assert(game.properties.every(property => property.owner === null
        || !game.players[property.owner].bankrupt));
      conserved(game);
    }
    assert(steps < 500, "match did not terminate");
    assert(game.round <= ROUND_LIMIT + 1);
    assert(game.winners.length >= 1);
    return game;
  }
  assert.deepEqual(match(), match());
});

test("AI observation describes the full public economy and indexed legal actions", () => {
  const game = newGame();
  const moves = choices(game);
  const offer = turn(game, moves);
  assert.equal(offer.choices[0].id, 0);
  assert.deepEqual(offer.choices[0], { id: 0, kind: "roll" });
  assert.equal(offer.state.board.length, 24);
  assert.equal(offer.state.worth[0], START_CASH);
  assert.equal(offer.state.rents.length, 24);
  assert.equal(observation(game).phase, "roll");
  assert(Buffer.byteLength(JSON.stringify(offer.state)) < 24 * 1024);
  assert(Buffer.byteLength(offer.instructions) < 1024);
});

test("saved checkpoints restore without sharing mutable state", async () => {
  const { restore } = await import("../dev.sailry.platform/desktop/game.js");
  const game = newGame();
  rollTwo(game);
  const restored = restore(game);
  assert.deepEqual(restored, game);
  assert(act(restored, { kind: "buy" }));
  assert.equal(game.properties[2].owner, null);
  for (const value of [null, {}, {...game, turn:1}, {...game, players:[]}, {...game, limit:99}])
    assert.throws(() => restore(value), /Invalid saved game/);
});
