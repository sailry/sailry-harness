import test from "node:test";
import assert from "node:assert/strict";
import { pattern, beats, legal, rank } from "../dev.sailry.platform/desktop/cards.js";
import { deal, bid, play, observation, choices, tablePlay, turn } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

function cards(...ranks) {
  const used = new Map();
  return ranks.map(value => {
    if (value >= 16) return value + 36;
    const suit = used.get(value) || 0;
    used.set(value, suit + 1);
    assert(suit < 4);
    return (value - 3) * 4 + suit;
  });
}
function random(seed) {
  return () => { seed = (1664525 * seed + 1013904223) >>> 0; return seed / 2 ** 32; };
}

test("all supported patterns and forbidden sequences", () => {
  for (const [ranks, expected] of [
    [[3], "single"], [[3, 3], "pair"], [[4, 4, 4], "triple"], [[4, 4, 4, 5], "tripleSingle"],
    [[4, 4, 4, 5, 5], "triplePair"], [[3, 4, 5, 6, 7], "straight"], [[3, 3, 4, 4, 5, 5], "pairs"],
    [[3, 3, 3, 4, 4, 4], "plane"], [[3, 3, 3, 4, 4, 4, 5, 6], "planeSingle"],
    [[3, 3, 3, 4, 4, 4, 5, 5], "planeSingle"], [[3, 3, 3, 4, 4, 4, 5, 5, 6, 6], "planePair"],
    [[3, 3, 3, 3], "bomb"], [[16, 17], "rocket"], [[3, 3, 3, 3, 4, 4], "fourSingle"],
    [[3, 3, 3, 3, 4, 4, 5, 5], "fourPair"],
  ]) assert.equal(pattern(cards(...ranks))?.kind, expected, ranks.join(","));
  for (const ranks of [[11, 12, 13, 14, 15], [3, 3, 4, 4], [14, 14, 14, 15, 15, 15],
    [3, 3, 3, 3, 4, 4, 4, 4], [3, 3, 3, 4, 4, 4, 4, 5], [3, 4, 6, 7, 8]]) {
    assert.equal(pattern(cards(...ranks)), null, ranks.join(","));
  }
  assert.equal(pattern([0, 0]), null);
  assert.equal(pattern([54]), null);
});

test("bombs, rockets, ranks and sequence lengths determine comparison", () => {
  const shape = (...ranks) => pattern(cards(...ranks));
  assert(beats(shape(4), shape(3)));
  assert(!beats(shape(3, 3), shape(4)));
  assert(beats(shape(3, 3, 3, 3), shape(15)));
  assert(!beats(shape(3, 3, 3, 3), shape(4, 4, 4, 4)));
  assert(beats(shape(16, 17), shape(15, 15, 15, 15)));
  assert(!beats(shape(16, 17), shape(16, 17)));
  assert(!beats(shape(4, 5, 6, 7, 8, 9), shape(3, 4, 5, 6, 7)));
});

test("dealing, bidding and hidden information", () => {
  const game = deal(random(12));
  assert.equal(new Set([...game.hands.flat(), ...game.bottom]).size, 54);
  assert.deepEqual(game.hands.map(hand => hand.length), [17, 17, 17]);
  assert.deepEqual(observation(game).bottom, []);
  assert(!("hands" in observation(game)));
  assert(!bid(game, 1, 1));
  assert(bid(game, 0, 1));
  assert(!bid(game, 1, 1));
  assert(bid(game, 1, 2));
  assert(bid(game, 2, 0));
  assert.equal(game.landlord, 1);
  assert.equal(game.turn, 1);
  assert.deepEqual(game.hands.map(hand => hand.length), [17, 20, 17]);
  assert(!play(game, 1, []));
  const passed = deal(random(4));
  for (let seat = 0; seat < 3; seat++) assert(bid(passed, seat, 0));
  assert.equal(passed.phase, "redeal");
});

test("two passes return the lead and reject duplicate or foreign cards", () => {
  const game = deal(random(5));
  bid(game, 0, 3);
  assert(!play(game, 0, [game.hands[0][0], game.hands[0][0]]));
  assert(!play(game, 0, [game.hands[1][0]]));
  assert(play(game, 0, [game.hands[0][0]]));
  assert(play(game, 1, []));
  assert(play(game, 2, []));
  assert.equal(game.turn, 0);
  assert.equal(game.last, null);
  assert(!play(game, 0, []));
});

test("legal move generation agrees with exhaustive small hands", () => {
  for (let seed = 1; seed <= 10; seed++) {
    const hand = deal(random(seed)).hands[0].slice(0, 10);
    const generated = new Set(legal(hand).map(move => move.map(rank).sort((a, b) => a - b).join(",")));
    for (let mask = 1; mask < 2 ** hand.length; mask++) {
      const move = hand.filter((_, index) => mask & (1 << index));
      if (pattern(move)) assert(generated.has(move.map(rank).sort((a, b) => a - b).join(",")));
    }
  }
});

test("complete seeded games conserve cards and finish with a valid winner", () => {
  for (let seed = 1; seed <= 30; seed++) {
    const rng = random(seed), game = deal(rng);
    bid(game, 0, 3);
    let turns = 0;
    while (game.phase === "play") {
      const options = choices(game);
      const move = options[Math.floor(rng() * options.length)];
      assert(play(game, game.turn, move));
      const all = [...game.hands.flat(), ...game.history.flatMap(entry => entry.cards)];
      assert.equal(all.length, 54);
      assert.equal(new Set(all).size, 54);
      assert(++turns < 200);
    }
    assert.equal(game.hands[game.winner].length, 0);
  }
});

test("model response must identify one offered move", () => {
  const moves = [[1], []];
  const provider = { model: "provider/test", kind: "provider" };
  assert.deepEqual(select({ Ok: { kind: "plugin_text",
    data: { text: '```json\n{"move":1}\n```', tokens: 1 } } }, provider, moves).move, []);
  for (const text of ['{"move":2}', '{"move":-1}', '{"move":"1"}', 'Play the ace']) {
    assert.throws(() => select({ Ok: { kind: "plugin_text",
      data: { text, tokens: 1 } } }, provider, moves));
  }
});

test("the table keeps only the latest cards through passes and replaces them on a new play", () => {
  const game = deal(random(5));
  assert.equal(tablePlay(game), null);
  bid(game, 0, 3);
  assert.equal(tablePlay(game), null);
  game.hands = [cards(3, 8), cards(4, 9), cards(5, 10)];
  const lead = [game.hands[0].find(card => rank(card) === 3)];
  assert(play(game, 0, lead));
  assert.deepEqual(tablePlay(game), { player: 0, cards: lead });
  assert(play(game, 1, []));
  assert.deepEqual(tablePlay(game), { player: 0, cards: lead });
  const response = [game.hands[2].find(card => rank(card) === 5)];
  assert(play(game, 2, response));
  assert.deepEqual(tablePlay(game), { player: 2, cards: response });
  assert(play(game, 0, []));
  assert.deepEqual(tablePlay(game), { player: 2, cards: response });
  assert(play(game, 1, []));
  assert.equal(game.last, null);
  assert.deepEqual(tablePlay(game), { player: 2, cards: response });
  const final = [...game.hands[2]];
  assert(play(game, 2, final));
  assert.equal(game.phase, "over");
  assert.deepEqual(tablePlay(game), { player: 2, cards: final });
  assert.equal(tablePlay(deal(random(6))), null);
});

test("model choices carry their exact ID and validated card type", () => {
  const game = deal(random(7), 0);
  bid(game, 0, 3);
  const moves = choices(game);
  const data = turn(game, moves);
  assert(!("hands" in data.state));
  for (const choice of data.choices) {
    const move = select({ Ok: { kind: "plugin_text",
      data: { text: JSON.stringify({ move: choice.id }), tokens: 1 } } },
    { model: "provider/test", kind: "provider" }, moves).move;
    assert.deepEqual(choice.cards, move.map(rank));
    assert.equal(choice.kind, pattern(move)?.kind || "pass");
  }
});

test("AI choices preserve card types, finishing moves and the option to pass", () => {
  for (let seed = 1; seed <= 40; seed++) {
    const game = deal(random(seed));
    bid(game, 0, 3);
    const all = legal(game.hands[0]);
    const moves = choices(game);
    assert.deepEqual(new Set(moves.map(move => pattern(move).kind)), new Set(all.map(move => pattern(move).kind)));
    assert(moves.length <= 28);
    for (const move of moves) assert(all.some(candidate => JSON.stringify(candidate) === JSON.stringify(move)));
  }
  const game = deal(random(1));
  bid(game, 0, 3);
  game.hands[0] = cards(3, 4, 5, 6, 7);
  assert(choices(game).some(move => move.length === 5));
  game.last = {player: 2, cards: cards(9), shape: pattern(cards(9))};
  assert.deepEqual(choices(game).at(-1), []);
});
