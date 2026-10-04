import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";

function host() {
  const pending = [], tasks = [], notified = [];
  const cx = { notify: () => notified.push(true),
    sleep: delay => new Promise(resolve => pending.push({ delay, resolve })),
    spawn: run => tasks.push(run(cx)) };
  return { cx, pending, tasks, notified };
}

async function step(pending, delay) {
  const next = pending.shift();
  assert.equal(next.delay, delay);
  next.resolve();
  await Promise.resolve();
}

const move = { fromRow: 6, fromColumn: 0, toRow: 5, toColumn: 0,
  piece: "P", captured: "." };

test("a move slides before impact, then settles and reveals the result", async () => {
  const motion = new Motion(), { cx, pending, tasks, notified } = host();
  motion.move(move, true, true, cx);
  assert.equal(motion.active.stage, 0);
  assert.equal(motion.resultVisible, false);
  await step(pending, 24);
  assert.equal(motion.active.stage, 1);
  await step(pending, 290);
  assert.equal(motion.active.stage, 2);
  await step(pending, 70);
  assert.equal(motion.active.stage, 3);
  assert.equal(motion.resultVisible, true);
  await step(pending, 330);
  await tasks[0];
  assert.equal(motion.active, null);
  assert.equal(notified.length, 4);
});

test("reset discards a delayed animation from the previous game", async () => {
  const motion = new Motion(), { cx, pending, tasks, notified } = host();
  motion.move(move, false, true, cx);
  motion.reset();
  await step(pending, 24);
  await tasks[0];
  assert.equal(motion.active, null);
  assert.equal(motion.resultVisible, false);
  assert.equal(notified.length, 0);
});

test("a newer move replaces an older slide", async () => {
  const motion = new Motion(), { cx, pending, tasks } = host();
  motion.move(move, false, false, cx);
  const first = motion.active.id;
  motion.move({ ...move, fromRow: 3, toRow: 4, piece: "p" }, false, false, cx);
  const second = motion.active.id;
  assert.notEqual(first, second);
  await step(pending, 24);
  await tasks[0];
  assert.equal(motion.active.id, second);
  assert.equal(motion.active.stage, 0);
  await step(pending, 24);
  assert.equal(motion.active.stage, 1);
});
