import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";
import { newGame, play } from "../dev.sailry.platform/desktop/game.js";

function clock() {
  const pending = [];
  const cx = { notify() {}, spawn: task => task(cx),
    sleep: () => new Promise(resolve => pending.push(resolve)) };
  return { cx, async tick() {
    for (const resolve of pending.splice(0)) resolve();
    await Promise.resolve();
  } };
}

test("flipping reveals the new face at the narrow midpoint without delaying play", async () => {
  const game = newGame(), motion = new Motion(), { cx, tick } = clock();
  motion.reset(game);
  assert.ok(play(game, 1, 2, 3));
  motion.update(game, cx);
  const flip = motion.discs.get("3-3");
  assert.equal(flip.from, 2);
  assert.equal(flip.value, 1);
  assert.equal(game.turn, 2);
  assert.equal(game.board[3][3], 1);
  await tick();
  assert.equal(flip.phase, 1);
  await tick();
  assert.equal(flip.phase, 2);
  await tick();
  assert.equal(motion.discs.size, 0);
});

test("a following move replaces an unfinished flip without stale cleanup", async () => {
  const game = newGame(), motion = new Motion(), { cx, tick } = clock();
  motion.reset(game);
  play(game, 1, 2, 3);
  motion.update(game, cx);
  const first = motion.discs.get("3-3");
  play(game, 2, 2, 2);
  motion.update(game, cx);
  const second = motion.discs.get("3-3");
  assert.notEqual(second, first);
  assert.equal(second.value, 2);
  await tick();
  await tick();
  assert.equal(motion.discs.get("3-3"), second);
  await tick();
  assert.equal(motion.discs.size, 0);
});

test("reset prevents previous flips and results from returning", async () => {
  const game = newGame(), motion = new Motion(), { cx, tick } = clock();
  motion.reset(game);
  play(game, 1, 2, 3);
  game.phase = "over";
  motion.update(game, cx);
  const fresh = newGame();
  motion.reset(fresh);
  await tick();
  assert.equal(motion.game, fresh);
  assert.equal(motion.discs.size, 0);
  assert.equal(motion.result, null);
});
