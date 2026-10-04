import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";

function clock() {
  const timers = [], tasks = [];
  const cx = { notify() {}, sleep(delay) { return new Promise(resolve => timers.push({delay, resolve})); }, spawn(run) { tasks.push(run(cx)); } };
  return { cx, timers, async step() { const pending = timers.splice(0); for (const timer of pending) timer.resolve(); await Promise.resolve(); await Promise.resolve(); } };
}
test("round replacement cancels earlier animation steps", async () => {
  const time = clock(), motion = new Motion();
  motion.begin(time.cx);
  await time.step();
  assert.equal(motion.dealing, 1);
  motion.begin(time.cx);
  await time.step();
  assert.equal(motion.dealing, 1);
  await time.step(); await time.step(); await time.step();
  assert.equal(motion.dealing, null);
});
test("victory particles finish without driving the game", async () => {
  const time = clock(), motion = new Motion();
  const game = { phase:"play", turn:1, history:[], last:null };
  motion.update(game,time.cx);
  assert.equal(time.timers.length,0);
  game.phase="over"; game.won=true;
  motion.update(game,time.cx);
  assert.equal(motion.resultReady,false);
  assert.equal(time.timers[0].delay,2000);
  await time.step();
  assert.equal(motion.resultReady,true);
  assert.equal(motion.fireworks,0);
  await time.step(); await time.step(); await time.step();
  assert.equal(motion.fireworks,null);
  assert.equal(game.history.length,0);
});
test("passes retain the last card animation without collecting or delaying the next turn", async () => {
  const time = clock(), motion = new Motion();
  const last = {player:1,cards:[3]};
  const game = {phase:"play",turn:2,history:[last],last};
  motion.update(game,time.cx);
  await time.step();
  assert.equal(motion.play,last);
  assert.equal(motion.entered,true);
  game.history.push({player:2,cards:[]},{player:0,cards:[]}); game.last=null;
  motion.update(game,time.cx);
  assert.equal(motion.play,last);
  assert.equal(motion.entered,true);
  assert.equal(time.timers.length,0);
  const next = {player:1,cards:[4]};
  game.history.push(next); game.last=next;
  motion.update(game,time.cx);
  assert.equal(motion.play,next);
  assert.equal(motion.entered,false);
  await time.step();
  assert.equal(motion.entered,true);
});
test("restarting during fireworks removes the old result effects", async () => {
  const time = clock(), motion = new Motion();
  motion.update({phase:"over", turn:0, history:[], last:null, won:true},time.cx);
  await time.step();
  assert.equal(motion.fireworks,0);
  motion.begin(time.cx);
  await time.step();
  assert.equal(motion.fireworks,null);
});

test("a new round cancels the pending result overlay", async () => {
  const time = clock(), motion = new Motion();
  motion.update({phase:"over", turn:0, history:[], last:null, won:false},time.cx);
  assert.equal(motion.resultReady,false);
  motion.begin(time.cx);
  await time.step();
  assert.equal(motion.resultReady,false);
  assert.equal(motion.fireworks,null);
});
