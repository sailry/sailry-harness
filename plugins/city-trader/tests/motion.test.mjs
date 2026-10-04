import test from "node:test";
import assert from "node:assert/strict";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";
import { act, newGame } from "../dev.sailry.platform/desktop/game.js";

function immediate(snapshot) {
  const tasks = [];
  const cx = { notify: () => snapshot?.(), sleep: async () => {},
    spawn: callback => { tasks.push(callback(cx)); } };
  return { cx, tasks };
}

async function drain(tasks) {
  while (tasks.length) await tasks.shift();
}

test("rolling reveals dice and advances the token one board tile at a time", async () => {
  const game = newGame(), motion = new Motion(), visited = [];
  const { cx, tasks } = immediate(() => visited.push(motion.positions[0]));
  motion.reset(game);
  assert.equal(act(game, { kind: "roll" }, () => 0.2), true);
  motion.update(game, cx);
  assert.equal(motion.moving, true);
  assert.equal(motion.positions[0], 0);
  const settled = motion.settle(cx);
  await drain(tasks);
  await settled;
  assert.deepEqual(motion.dice, [2, 2]);
  assert.deepEqual(visited.filter((tile, index, all) => index === 0 || tile !== all[index - 1]),
    [0, 1, 2, 3, 4]);
  assert.equal(motion.positions[0], 4);
  assert.equal(motion.moving, false);
});

test("passing Start animates the wrap and keeps cash effects independent", async () => {
  const game = newGame(), motion = new Motion(), visited = [];
  game.players[0].position = game.board.length - 1;
  const { cx, tasks } = immediate(() => visited.push(motion.positions[0]));
  motion.reset(game);
  assert.equal(act(game, { kind: "roll" }, () => 0), true);
  motion.update(game, cx);
  assert.equal(game.players[0].cash, 1400);
  assert.equal(motion.cashDelta[0].amount, 200);
  // A later visual flash must not cancel the roll's settlement promise.
  motion.flashTile(1, cx);
  await drain(tasks);
  assert.ok(visited.includes(0));
  assert.ok(visited.includes(1));
  assert.equal(motion.positions[0], 1);
  assert.equal(motion.moving, false);
  await motion.settle(cx);
});

test("reset resolves and invalidates a pending roll from the prior game", async () => {
  const oldGame = newGame(), fresh = newGame(), motion = new Motion();
  const waits = [], tasks = [];
  const cx = { notify() {}, sleep: () => new Promise(resolve => waits.push(resolve)),
    spawn: callback => { tasks.push(callback(cx)); } };
  motion.reset(oldGame);
  assert.equal(act(oldGame, { kind: "roll" }, () => 0), true);
  motion.update(oldGame, cx);
  const stale = motion.settle(cx);
  motion.reset(fresh);
  await stale;
  assert.equal(motion.game, fresh);
  assert.equal(motion.moving, false);
  waits.shift()();
  await Promise.all(tasks);
  assert.equal(motion.positions[0], 0);
});

test("the final standing enters after movement settles", async () => {
  const game = newGame(), motion = new Motion();
  const { cx, tasks } = immediate();
  motion.reset(game);
  game.phase = "over";
  motion.update(game, cx);
  assert.equal(motion.resultVisible, false);
  await drain(tasks);
  assert.equal(motion.resultVisible, true);
});


test("restored results are visible without replaying the completed match",()=>{
  const motion=new Motion(), game=newGame();
  game.phase="over";
  motion.reset(game);
  assert.equal(motion.resultVisible,true);
  assert.equal(motion.moving,false);
});

test("each rolled result settles through its own frames before any board step", async () => {
  for (let face = 1; face <= 6; face++) {
    const game = newGame(), motion = new Motion(), frames = [];
    const { cx, tasks } = immediate(() => {
      if (motion.rolling) {
        assert.equal(motion.positions[0], 0);
        if (frames.at(-1)?.[0] !== motion.settling || frames.at(-1)?.[1] !== motion.diceFrame)
          frames.push([motion.settling, motion.diceFrame, [...motion.dice]]);
      }
    });
    motion.reset(game);
    act(game, { kind: "roll" }, () => (face - 0.5) / 6);
    motion.update(game, cx);
    await drain(tasks);
    assert.deepEqual(frames.filter(([settling]) => !settling).map(([, frame]) => frame), Array.from({ length: 20 }, (_, i) => i));
    const landing = frames.filter(([settling]) => settling);
    assert.deepEqual(landing.map(([, frame]) => frame), Array.from({ length: 8 }, (_, i) => i));
    assert.ok(landing.every(([, , dice]) => dice[0] === face && dice[1] === face));
    assert.deepEqual(motion.dice, [face, face]);
    assert.equal(motion.rolling, false);
  }
});
