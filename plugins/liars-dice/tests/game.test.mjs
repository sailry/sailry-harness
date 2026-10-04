import test from "node:test";
import assert from "node:assert/strict";
import { active, bid, challenge, choices, deal, legalBid, nextActive, nextRound, observation, turn } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

function random(seed) {
  return () => { seed = (1664525 * seed + 1013904223) >>> 0; return seed / 2 ** 32; };
}

test("deal keeps each hand private and rolls the active dice", () => {
  const game = deal(random(12));
  assert.deepEqual(game.counts, [5, 5, 5]);
  assert.deepEqual(game.hands.map(hand => hand.length), [5, 5, 5]);
  assert(game.hands.flat().every(die => die >= 1 && die <= 6));
  const seen = observation(game);
  assert.deepEqual(seen.hand, game.hands[0]);
  assert(!("hands" in seen));
  assert(!("future" in seen));
  assert.throws(() => deal(random(1), [0, 1, 0]));
  assert.throws(() => deal(random(1), [5, 5, 5], -1));
});

test("bids increase by quantity then face and skip eliminated seats", () => {
  const game = deal(random(2), [5, 0, 5], 0);
  assert.equal(nextActive(game.counts, 0), 2);
  assert(!bid(game, 1, 1, 1));
  assert(bid(game, 0, 2, 4));
  assert.equal(game.turn, 2);
  assert(!legalBid(game, 2, 4));
  assert(!legalBid(game, 2, 3));
  assert(legalBid(game, 2, 5));
  assert(legalBid(game, 3, 1));
  assert(!bid(game, 2, 2, 4));
  assert(bid(game, 2, 2, 5));
  assert.equal(game.turn, 0);
  assert(!legalBid(game, 11, 1));
  assert.equal(game.history.length, 2);
});

test("challenge counts exact faces with no wild ones and loser opens next round", () => {
  const game = deal(random(3), [2, 1, 1], 0);
  game.hands = [[1, 2], [1], [2]];
  assert(!challenge(game, 0));
  assert(bid(game, 0, 3, 2));
  assert(challenge(game, 1));
  assert.equal(game.result.actual, 2);
  assert.equal(game.result.loser, 0);
  assert.deepEqual(game.counts, [1, 1, 1]);
  assert.equal(game.result.nextStarter, 0);
  assert.equal(game.phase, "round_over");
  const next = nextRound(game, random(4));
  assert.deepEqual(next.counts, [1, 1, 1]);
  assert.equal(next.turn, 0);
  assert.equal(next.round, 2);
  assert.equal(next.currentBid, null);
  assert(!nextRound(next));
});

test("elimination transfers first move and last survivor wins", () => {
  const game = deal(random(5), [1, 1, 1], 0);
  game.hands = [[1], [2], [3]];
  assert(bid(game, 0, 2, 6));
  assert(challenge(game, 1));
  assert.deepEqual(game.counts, [0, 1, 1]);
  assert.equal(game.result.nextStarter, 1);
  const next = nextRound(game, random(6));
  assert.equal(next.turn, 1);
  assert.deepEqual(active(next.counts), [1, 2]);
  next.hands = [[], [1], [2]];
  assert(bid(next, 1, 2, 6));
  assert(challenge(next, 2));
  assert.equal(next.phase, "match_over");
  assert.equal(next.result.winner, 2);
  assert.deepEqual(next.counts, [0, 0, 1]);
  assert(!nextRound(next));

  const held = deal(random(7), [1, 1, 0], 0);
  held.hands = [[6], [6], []];
  assert(bid(held, 0, 2, 6));
  assert(challenge(held, 1));
  assert.equal(held.result.loser, 1);
  assert.equal(held.result.winner, 0);
});

test("model choices are legal and expose only its own dice", () => {
  const game = deal(random(8), [5, 5, 5], 1);
  const opening = choices(game);
  assert.deepEqual(opening[0], { kind: "bid", quantity: 1, face: 1 });
  assert.equal(opening.length, 90);
  assert(bid(game, 1, 2, 3));
  const moves = choices(game);
  assert.deepEqual(moves[0], { kind: "challenge" });
  assert(moves.slice(1).every(move => legalBid(game, move.quantity, move.face)));
  const provider = { model: "provider/test", kind: "provider" };
  const data = JSON.parse(command(provider, turn(game, moves)).data.prompt.split("\n").at(-1));
  assert.equal(data.state.player, 2);
  assert.deepEqual(data.state.hand, game.hands[2]);
  assert(!("hands" in data.state));
  assert(!("hands" in data.choices));
  assert.deepEqual(select({ Ok: { kind: "plugin_text",
    data: { text: '```json\n{"move":0}\n```', tokens: 1 } } }, provider, moves).move, moves[0]);
  for (const response of ['{"move":-1}', '{"move":100}', '{"move":"0"}', "Challenge"]) {
    assert.throws(() => select({ Ok: { kind: "plugin_text",
      data: { text: response, tokens: 1 } } }, provider, moves));
  }
});

test("deterministic matches lose one die per challenge and finish", () => {
  for (let seed = 1; seed <= 20; seed++) {
    let game = deal(random(seed));
    let challenges = 0;
    while (game.phase !== "match_over") {
      const before = game.counts.reduce((sum, count) => sum + count, 0);
      assert(bid(game, game.turn, 1, 1));
      assert(challenge(game, game.turn));
      assert.equal(game.counts.reduce((sum, count) => sum + count, 0), before - 1);
      assert(++challenges <= 14);
      if (game.phase === "round_over") game = nextRound(game, random(seed + challenges));
    }
    assert.equal(active(game.counts).length, 1);
    assert.equal(game.result.winner, active(game.counts)[0]);
  }
});
