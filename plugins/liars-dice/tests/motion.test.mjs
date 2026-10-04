import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";

function immediate(tasks) {
  const cx = { notify() {}, sleep: async () => {}, spawn: callback => { tasks.push(callback(cx)); } };
  return cx;
}

async function drain(tasks) { while (tasks.length) await tasks.shift(); }

test("covered dice land, reveal and show the challenge result", async () => {
  const tasks = [], cx = immediate(tasks), motion = new Motion();
  const game = { history: [], result: null };
  motion.begin(game, cx);
  assert.equal(motion.rolling, true);
  assert.equal(motion.handShown, false);
  await drain(tasks);
  assert.equal(motion.rolling, false);
  assert.equal(motion.handShown, true);
  game.history.push({ kind: "bid" });
  motion.update(game, cx);
  assert.equal(motion.bidPulse, true);
  await drain(tasks);
  assert.equal(motion.bidPulse, false);
  game.history.push({ kind: "challenge" });
  game.result = { actual: 2 };
  motion.update(game, cx);
  assert.equal(motion.revealSeats, 0);
  assert.equal(motion.resultShown, false);
  await drain(tasks);
  assert.equal(motion.revealSeats, 2);
  assert.equal(motion.resultShown, true);
});

test("a new round ignores stale reveal timers", async () => {
  const waits = [], tasks = [];
  const cx = { notify() {}, sleep: () => new Promise(resolve => waits.push(resolve)),
    spawn: callback => { tasks.push(callback(cx)); } };
  const motion = new Motion();
  const old = { history: [{ kind: "challenge" }], result: { actual: 0 } };
  motion.begin(old, cx);
  motion.update(old, cx);
  motion.begin({ history: [], result: null }, cx);
  for (let index = 0; index < 2; index++) {
    waits.shift()();
    await Promise.resolve();
  }
  assert.equal(motion.revealSeats, 0);
  assert.equal(motion.resultShown, false);
  while (waits.length) {
    waits.shift()();
    await Promise.resolve();
  }
  await drain(tasks);
  assert.equal(motion.handShown, true);
  assert.equal(motion.resultShown, false);
});
