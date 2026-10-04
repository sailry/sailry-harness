import test from "node:test";
import assert from "node:assert/strict";
import { pattern, rank, sort } from "../dev.sailry.platform/desktop/cards.js";
import { sweep } from "../dev.sailry.platform/desktop/selection.js";

function cards(...ranks) {
  const used = new Map();
  return sort(ranks.map(value => {
    const suit = used.get(value) || 0;
    used.set(value, suit + 1);
    return value >= 16 ? value + 36 : (value - 3) * 4 + suit;
  }));
}

test("a new pair replaces an incompatible selected single", () => {
  const hand = cards(3, 9, 9, 12);
  const selected = [hand.at(-1)];
  assert.deepEqual(sweep(hand, selected, 1, 2).map(rank), [9, 9]);
  assert.deepEqual(sweep(hand, selected, 2, 1).map(rank), [9, 9]);
  assert.deepEqual(selected, [hand.at(-1)]);
});

test("sweeps recognize complete legal card types", () => {
  for (const [ranks, kind] of [
    [[9, 9], "pair"], [[4, 5, 6, 7, 8], "straight"],
    [[4, 4, 5, 5, 6, 6], "pairs"], [[8, 8, 8], "triple"],
    [[8, 8, 8, 8], "bomb"], [[16, 17], "rocket"],
    [[5, 5, 5, 6, 6, 6], "plane"],
  ]) {
    const hand = cards(...ranks);
    for (const [start, end] of [[0, hand.length - 1], [hand.length - 1, 0]]) {
      const selected = sweep(hand, [], start, end);
      assert.deepEqual(selected, hand);
      assert.equal(pattern(selected).kind, kind);
    }
  }
});

test("duplicate ranks in a sweep do not break a straight", () => {
  const hand = cards(4, 5, 6, 6, 7, 8);
  const selected = sweep(hand, [], 0, hand.length - 1);
  assert.equal(pattern(selected).kind, "straight");
  assert.deepEqual(selected.map(rank), [8, 7, 6, 5, 4]);
  assert(selected.every(card => hand.includes(card)));
});

test("compatible selections combine into a straight or an attached triple", () => {
  const hand = cards(3, 4, 5, 6, 7);
  assert.deepEqual(sweep(hand, [hand.at(-1)], 0, 3), hand);
  const attached = cards(3, 9, 9, 9);
  assert.deepEqual(sweep(attached, [attached.at(-1)], 0, 2), attached);
});

test("dragging from a raised card lowers only the swept range", () => {
  const hand = cards(3, 9, 9, 12);
  assert.deepEqual(sweep(hand, hand, 1, 2), [hand[0], hand[3]]);
  assert.deepEqual(sweep(hand, hand, 2, 1), [hand[0], hand[3]]);
});

test("unrelated ranks choose the endpoint without adding unseen cards", () => {
  const hand = cards(3, 7, 11);
  assert.deepEqual(sweep(hand, [], 0, 2), [hand[2]]);
  assert.deepEqual(sweep(hand, [], 2, 0), [hand[0]]);
});
