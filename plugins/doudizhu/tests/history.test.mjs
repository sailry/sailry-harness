import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { deal, bid, play } from "../dev.sailry.platform/desktop/game.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/history.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const scope = {};
vm.runInNewContext(`${source}\nglobalThis.recent = recentPlays;`, scope);

test("records played cards without bidding or passing, newest first", () => {
  const game = deal(() => 0.5);
  assert.equal(scope.recent(game).length, 0);
  assert(bid(game, 0, 3));
  assert.equal(scope.recent(game).length, 0);
  const first = game.hands[0].at(-1);
  assert(play(game, 0, [first]));
  assert(play(game, 1, []));
  assert(play(game, 2, []));
  const next = game.hands[0].at(-1);
  assert(play(game, 0, [next]));
  const before = JSON.stringify(game);
  const entries = scope.recent(game);
  assert.deepEqual(Array.from(entries, entry => entry.index), [3, 0]);
  assert.deepEqual(Array.from(entries, entry => Array.from(entry.cards)), [[next], [first]]);
  assert(entries.every(entry => entry.player === 0));
  assert.equal(JSON.stringify(game), before);
});

test("limits the visible record while preserving the full round history", () => {
  const game = { history: Array.from({ length: 100 }, (_, index) =>
    ({ player: index % 3, cards: index % 2 ? [] : [index % 52] })) };
  const entries = scope.recent(game);
  assert.equal(entries.length, 40);
  assert.equal(entries[0].index, 98);
  assert.equal(entries.at(-1).index, 20);
  assert.equal(game.history.length, 100);
});
