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

test("consecutive turns keep both stone arrivals without delaying the game", async () => {
  const game = newGame(), motion = new Motion(), { cx, tick } = clock();
  motion.reset(game);
  play(game, 1, 7, 7);
  motion.update(game, cx);
  play(game, 2, 7, 8);
  motion.update(game, cx);
  assert.equal(game.moves.length, 2);
  assert.equal(game.turn, 1);
  assert.equal(motion.stones.size, 2);
  await tick();
  assert.ok([...motion.stones.values()].every(stone => stone.ready));
  await tick();
  assert.equal(motion.stones.size, 0);
});

test("reset discards old placements and winning feedback", async () => {
  const game = newGame(), motion = new Motion(), { cx, tick } = clock();
  motion.reset(game);
  for (let column = 0; column < 4; column++) {
    play(game, 1, 7, column);
    play(game, 2, 8, column);
  }
  play(game, 1, 7, 4);
  motion.update(game, cx);
  assert.equal(game.phase, "over");
  assert.equal(motion.result.visible, false);
  const fresh = newGame();
  motion.reset(fresh);
  await tick();
  assert.equal(motion.game, fresh);
  assert.equal(motion.result, null);
  assert.equal(motion.stones.size, 0);
});
