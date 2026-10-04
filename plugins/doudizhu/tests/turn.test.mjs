import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import * as game from "../dev.sailry.platform/desktop/game.js";
import * as model from "../../tests/model.mjs";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default class", "globalThis.Game = class");

for (const pending of [null, "admitted"]) test(`forced turns ${pending ? "recover an admitted result" : "do not invoke a model"}`, async () => {
  let requests = 0, forgotten = 0, task;
  const scope = { ...game, ...model, View: class {},
    prepare: () => assert.fail("forced actions must not prepare a generation"),
    execute: async id => {
      assert.equal(id, "admitted"); requests++;
      return JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 12 } } });
    },
    forget: id => { assert.equal(id, "admitted"); forgotten++; }, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.Game();
  Object.assign(view, { game: game.deal(() => 0.5), pending, busy: false, tokens: 0, score: 0,
    roundPlayers: [{ model: "first/model", kind: "provider" }, { model: "second/model", kind: "provider" }] });
  game.bid(view.game, 0, 3);
  view.game.hands = [[51, 0], [1], [2]];
  assert(game.play(view.game, 0, [51]));
  const cx = { notify() {}, timer: { every: () => ({ cancel() {} }) }, spawn: callback => { task = callback(cx); } };
  view.advance(cx);
  await task;
  assert.equal(view.game.turn, 0);
  assert.equal(view.game.last, null);
  assert.equal(view.game.history.length, 3);
  assert.equal(view.error, null);
  assert.equal(view.busy, false);
  assert.equal(requests, pending ? 1 : 0);
  assert.equal(forgotten, requests);
  assert.equal(view.tokens, pending ? 12 : 0);
});

test("each opponent uses its own model and effort", async () => {
  const sent = [];
  let task;
  const scope = { ...game, ...model, View: class {},
    prepare: value => { sent.push(JSON.parse(value)); return `request-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 1 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.Game();
  Object.assign(view, { game: game.deal(() => 0.5, 1), pending: null, busy: false, tokens: 0, score: 0,
    roundPlayers: [{ model: "first/model", kind: "provider", effort: "high" },
      { model: "second/model", kind: "provider", effort: "none" }] });
  const cx = { notify() {}, timer: { every: () => ({ cancel() {} }) }, spawn: callback => { task = callback(cx); } };
  view.advance(cx);
  await task;
  assert.equal(view.error, null);
  assert.deepEqual(sent.map(request => [request.data.model, request.data.effort]), [["first/model", "high"], ["second/model", "none"]]);
  assert.equal(view.game.turn, 0);
});

test("provider seats use public turn state and can retry an invalid move", async () => {
  const sent = [], notices = [];
  let task, attempts = 0;
  const scope = { ...game, ...model, View: class {},
    toast: value => notices.push(value),
    prepare: value => { sent.push(JSON.parse(value)); return `request-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text",
      data: { text: JSON.stringify({ move: attempts++ === 0 ? "01" : 0 }), tokens: 2 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.Game();
  const match = game.deal(() => 0.5, 1);
  Object.assign(view, { game: match, pending: null, busy: false, tokens: 0, score: 0, text: messages("en"),
    roundPlayers: [{ model: "first/ddz", kind: "provider" },
      { model: "second/ddz", kind: "provider" }] });
  const cx = { notify() {}, timer: { every: () => ({ cancel() {} }) },
    spawn: callback => { task = callback(cx); } };
  view.advance(cx);
  await task;
  assert.equal(view.error, "invalidResponse");
  assert.equal(notices.length, 1);
  assert.equal(notices[0].message, view.text.invalidResponse);
  assert.equal(match.turn, 1);
  assert.deepEqual(match.bids, []);
  view.advance(cx);
  await task;
  assert.equal(view.error, null);
  assert.equal(notices.length, 1);
  assert.equal(match.turn, 0);
  assert.equal(view.tokens, 4);
  assert.deepEqual(sent.map(request => request.data.model),
    ["first/ddz", "first/ddz", "second/ddz"]);
  const turns = sent.map(request => JSON.parse(request.data.prompt.split("\n").at(-1)));
  assert(sent.every(request => request.kind === "generate_plugin_text" && !("effort" in request.data)));
  assert(turns.every(turn => !("hands" in turn.state)));
  assert.deepEqual(turns.map(turn => turn.state.player), [1, 1, 2]);
  assert.deepEqual(turns[0].state.bottom, []);
  assert.deepEqual(turns[0].choices[0], { id: 0, bid: 0 });
});

test("a completed trick immediately accepts the next human play", async () => {
  const tasks = [];
  const scope = { ...game, ...model, View: class {},
    prepare: () => assert.fail("forced passes need no model request"),
  };
  vm.runInNewContext(source, scope);
  const view = new scope.Game();
  Object.assign(view, { game: game.deal(() => 0.5), pending: null, busy: false, tokens: 0, score: 0,
    motion: new Motion() });
  game.bid(view.game, 0, 3);
  view.game.hands = [[51, 0], [1], [2]];
  const cx = { notify() {}, sleep: async () => {},
    timer: { every: () => ({ cancel() {} }) }, spawn: run => tasks.push(run(cx)) };
  view.act([51], cx);
  await Promise.all(tasks);
  assert.equal(view.game.turn, 0);
  assert.equal(view.game.last, null);
  assert.equal(view.busy, false);
  assert.deepEqual(game.tablePlay(view.game), { player: 0, cards: [51] });
  view.act([0], cx);
  assert.equal(view.game.phase, "over");
  assert.deepEqual(game.tablePlay(view.game), { player: 0, cards: [0] });
  await Promise.all(tasks);
});

test("reset redeals and ignores the old model response", async () => {
  let reply, task, cancelled = 0;
  const forgotten = [];
  const scope = { ...game, ...model, View: class {},
    prepare: () => "old-turn",
    execute: () => new Promise(resolve => reply = resolve),
    forget: id => forgotten.push(id), outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.Game();
  const players = [{ model: "first/model", kind: "provider" },
    { model: "second/model", kind: "provider" }];
  Object.assign(view, { game: game.deal(() => 0.5, 1), pending: null, busy: false, tokens: 0, score: 30,
    round: 3, available: true, loading: false, players, roundPlayers: players });
  const cx = { notify() {}, timer: { every: () => ({ cancel() { cancelled++; } }) }, spawn: run => task = run(cx) };
  view.advance(cx);
  assert.equal(view.busy, true);
  view.reset(cx);
  const fresh = view.game;
  assert.equal(view.score, 0);
  assert.equal(view.busy, false);
  assert.equal(fresh.phase, "bid");
  assert.equal(fresh.turn, 0);
  assert.deepEqual([...fresh.hands.map(hand => hand.length)], [17, 17, 17]);
  assert.equal(cancelled, 1);
  reply(JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":3}', tokens: 1 } } }));
  await task;
  assert.equal(view.game, fresh);
  assert.equal(view.game.bids.length, 0);
  assert.equal(view.pending, null);
  assert.equal(view.error, null);
  assert.equal(view.score, 0);
  assert.deepEqual(forgotten, ["old-turn"]);
});
