import test from "node:test";
import assert from "node:assert/strict";
import { compare, evaluate } from "../dev.sailry.platform/desktop/cards.js";
import { act, choices, deal, legal, observation, turn, visibleBoard } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

const card = (rank, suit = 0) => (rank - 2) * 4 + suit;
function random(seed) {
  return () => { seed = (1664525 * seed + 1013904223) >>> 0; return seed / 2 ** 32; };
}

test("seven-card evaluation selects the best five and breaks ties", () => {
  const categories = [
    [card(14), card(13, 1), card(9, 2), card(5, 3), card(2)],
    [card(14), card(14, 1), card(13, 2), card(9, 3), card(2)],
    [card(14), card(14, 1), card(13, 2), card(13, 3), card(2)],
    [card(14), card(14, 1), card(14, 2), card(13, 3), card(2)],
    [card(9), card(10, 1), card(11, 2), card(12, 3), card(13)],
    [card(14), card(11), card(9), card(5), card(2)],
    [card(14), card(14, 1), card(14, 2), card(13), card(13, 1)],
    [card(14), card(14, 1), card(14, 2), card(14, 3), card(13)],
    [card(10), card(11), card(12), card(13), card(14)],
  ];
  assert.deepEqual(categories.map(cards => evaluate(cards)[0]), [0, 1, 2, 3, 4, 5, 6, 7, 8]);
  const royal = [10, 11, 12, 13, 14].map(rank => card(rank));
  assert.deepEqual(evaluate([...royal, card(2, 1), card(2, 2)]), [8, 14]);
  assert.deepEqual(evaluate([card(14), card(2, 1), card(3, 2), card(4, 3), card(5), card(9), card(13)]), [4, 5]);
  assert.deepEqual(evaluate([card(14), card(14, 1), card(14, 2), card(13), card(13, 1), card(13, 2), card(2)]), [6, 14, 13]);
  assert(compare(evaluate(royal), evaluate([card(9), card(10), card(11), card(12), card(13)])) > 0);
  const first = evaluate([card(12), card(12, 1), card(8), card(8, 1), card(14), card(3), card(2)]);
  const second = evaluate([card(12), card(12, 1), card(8), card(8, 1), card(13), card(3), card(2)]);
  assert(compare(first, second) > 0);
  assert.equal(compare(first, first), 0);
  assert(compare(evaluate([card(14), card(14, 1), card(13), card(9), card(2)]),
    evaluate([card(14), card(14, 1), card(12), card(11), card(3)])) > 0);
});

test("heads-up blinds, order, streets and hidden cards", () => {
  const game = deal(random(6));
  assert.equal(game.dealer, 0);
  assert.equal(game.turn, 0);
  assert.deepEqual(game.streetBets, [5, 10]);
  assert.deepEqual(game.stacks, [995, 990]);
  assert.equal(game.pot, 15);
  assert.equal(new Set([...game.hands.flat(), ...game.board]).size, 9);
  assert.deepEqual(visibleBoard(game), []);
  assert.deepEqual(observation(game).hand, game.hands[0].map(card => observationCard(card)));
  const exposed = JSON.stringify(observation(game));
  assert(!exposed.includes(JSON.stringify(game.hands[1])));
  assert(!("hands" in observation(game)));
  assert(!("board" in observation(game) && observation(game).board.length));
  assert(!act(game, 1, { kind: "check" }));
  assert(!act(game, 0, { kind: "check" }));
  assert(act(game, 0, { kind: "call" }));
  assert.equal(game.turn, 1);
  assert(act(game, 1, { kind: "check" }));
  assert.equal(game.stage, "flop");
  assert.equal(game.turn, 1);
  assert.equal(visibleBoard(game).length, 3);
  assert(act(game, 1, { kind: "check" }));
  assert(act(game, 0, { kind: "check" }));
  assert.equal(game.stage, "turn");
  assert.equal(visibleBoard(game).length, 4);
});

function observationCard(value) {
  const rank = Math.floor(value / 4) + 2;
  return `${["2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A"][rank - 2]}${["♠", "♥", "♣", "♦"][value % 4]}`;
}

test("raises honor minimums, stacks and current actor", () => {
  const game = deal(random(7));
  assert.deepEqual([legal(game).toCall, legal(game).minTo, legal(game).maxTo], [5, 20, 1000]);
  assert(!act(game, 0, { kind: "raise", to: 15 }));
  assert(!act(game, 0, { kind: "raise", to: 1001 }));
  assert(!act(game, 0, { kind: "raise", to: 20.5 }));
  assert(act(game, 0, { kind: "raise", to: 20 }));
  assert.equal(game.turn, 1);
  assert.equal(legal(game).toCall, 10);
  assert.equal(legal(game).minTo, 30);
  assert(!act(game, 0, { kind: "call" }));
  assert(act(game, 1, { kind: "call" }));
  assert.equal(game.stage, "flop");
});

test("overbet against a short stack refunds uncalled chips", () => {
  const game = deal(random(8), [1000, 15]);
  assert(act(game, 0, { kind: "call" }));
  assert(act(game, 1, { kind: "check" }));
  assert(act(game, 1, { kind: "check" }));
  assert.equal(legal(game).minTo, 10);
  assert.equal(legal(game).maxTo, 990);
  assert(act(game, 0, { kind: "raise", to: 100 }));
  assert(act(game, 1, { kind: "call" }));
  assert.equal(game.phase, "over");
  assert.equal(game.settlement.pot, 30);
  assert.equal(game.stacks[0] + game.stacks[1], 1015);
  assert.equal(game.shown, 5);
});

test("short all-in raise is allowed and tie splits the pot", () => {
  const game = deal(random(9), [15, 1000]);
  assert(act(game, 0, { kind: "call" }));
  assert(act(game, 1, { kind: "check" }));
  assert(act(game, 1, { kind: "check" }));
  assert.deepEqual([legal(game).minTo, legal(game).maxTo, legal(game).canShortRaise], [10, 5, true]);
  assert(act(game, 0, { kind: "raise", to: 5 }));
  assert(act(game, 1, { kind: "call" }));
  assert.equal(game.phase, "over");
  assert.equal(game.stacks[0] + game.stacks[1], 1015);

  const tied = deal(random(10));
  tied.hands = [[card(2, 1), card(3, 1)], [card(4, 2), card(5, 2)]];
  tied.board = [card(10), card(11), card(12), card(13), card(14)];
  assert(act(tied, 0, { kind: "call" }));
  assert(act(tied, 1, { kind: "check" }));
  for (const stage of ["flop", "turn", "river"]) {
    assert.equal(tied.stage, stage);
    assert(act(tied, 1, { kind: "check" }));
    assert(act(tied, 0, { kind: "check" }));
  }
  assert.equal(tied.phase, "over");
  assert.equal(tied.settlement.winner, null);
  assert.deepEqual(tied.stacks, [1000, 1000]);
  assert.equal(tied.settlement.pot, 20);
});

test("fold hides the board and pays the pot", () => {
  const game = deal(random(11));
  assert(act(game, 0, { kind: "fold" }));
  assert.equal(game.phase, "over");
  assert.equal(game.settlement.winner, 1);
  assert.equal(game.settlement.reason, "fold");
  assert.equal(game.settlement.pot, 15);
  assert.equal(game.shown, 0);
  assert.deepEqual(game.stacks, [995, 1005]);
});

test("model receives only visible information and must select an offered action", () => {
  const game = deal(random(12));
  assert(act(game, 0, { kind: "call" }));
  const moves = choices(game);
  assert.deepEqual(moves[0], { kind: "check" });
  assert(moves.some(move => move.kind === "raise" && move.to === 30));
  const provider = { model: "provider/test", kind: "provider" };
  const payload = JSON.parse(command(provider, turn(game, moves)).data.prompt.split("\n").at(-1));
  assert.deepEqual(payload.state.hand, game.hands[1].map(observationCard));
  assert.deepEqual(payload.state.board, []);
  assert(!("hands" in payload.state));
  assert.deepEqual(select({ Ok: { kind: "plugin_text",
    data: { text: '```json\n{"move":0}\n```', tokens: 1 } } }, provider, moves).move, moves[0]);
  for (const answer of ['{"move":-1}', '{"move":99}', '{"move":"0"}', "check"]) {
    assert.throws(() => select({ Ok: { kind: "plugin_text",
      data: { text: answer, tokens: 1 } } }, provider, moves));
  }
});

test("seeded complete games preserve all chips and eventually finish", () => {
  for (let seed = 1; seed <= 40; seed++) {
    const game = deal(random(seed));
    let actions = 0;
    while (game.phase === "betting") {
      const moves = choices(game);
      const move = moves.find(move => move.kind === "check" || move.kind === "call");
      assert(move);
      assert(act(game, game.turn, move));
      assert(++actions <= 8);
      assert.equal(game.stacks[0] + game.stacks[1] + game.pot, 2000);
    }
    assert.equal(game.stacks[0] + game.stacks[1], 2000);
    assert.equal(game.shown, 5);
  }
});
