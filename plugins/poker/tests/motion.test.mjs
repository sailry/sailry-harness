import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";

function immediate(tasks) {
  const cx = { notify() {}, sleep: async () => {}, spawn: callback => { tasks.push(callback(cx)); } };
  return cx;
}

async function drain(tasks) { while (tasks.length) await tasks.shift(); }

test("deal, board, bet and result targets finish without changing game state", async () => {
  const tasks = [], cx = immediate(tasks), motion = new Motion();
  const game = { shown: 0, history: [], phase: "betting" };
  motion.begin(game, cx);
  assert.equal(motion.holeVisible, false);
  await drain(tasks);
  assert.equal(motion.holeVisible, true);
  game.shown = 3;
  game.history.push({ player: 0, kind: "call", amount: 5 });
  motion.update(game, cx);
  assert.equal(motion.boardVisible, 0);
  assert.equal(motion.potPulse, true);
  await drain(tasks);
  assert.equal(motion.boardVisible, 3);
  assert.equal(motion.potPulse, false);
  assert.equal(motion.cue, null);
  game.shown = 5;
  game.phase = "over";
  motion.update(game, cx);
  assert.equal(motion.resultShown, false);
  await drain(tasks);
  assert.equal(motion.boardVisible, 5);
  assert.equal(motion.resultShown, true);
  assert.equal(game.shown, 5);
});

test("a new deal invalidates the prior deal timer", async () => {
  const waits = [], tasks = [];
  const cx = { notify() {}, sleep: () => new Promise(resolve => waits.push(resolve)),
    spawn: callback => { tasks.push(callback(cx)); } };
  const motion = new Motion();
  motion.begin({ shown: 0, history: [], phase: "betting" }, cx);
  motion.begin({ shown: 0, history: [], phase: "betting" }, cx);
  waits.shift()();
  await Promise.resolve();
  assert.equal(motion.holeVisible, false);
  waits.shift()();
  await drain(tasks);
  assert.equal(motion.holeVisible, true);
});
