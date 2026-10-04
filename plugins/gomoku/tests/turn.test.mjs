import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import * as game from "../dev.sailry.platform/desktop/game.js";
import * as model from "../../tests/model.mjs";
import { Motion } from "../dev.sailry.platform/desktop/motion.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default class", "globalThis.Game = class");

function context(overrides = {}) {
  const scope = { ...game, ...model, Motion, View: class {},
    prepare: () => assert.fail("unexpected model request"),
    execute: () => assert.fail("unexpected model execution"),
    forget: () => {}, outcome: () => assert.fail("unexpected outcome query"), ...overrides };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  return scope;
}

function host(tasks = []) {
  const cx = { notify() {}, sleep: async () => {},
    timer: { every: () => ({ cancel() {} }) },
    spawn: run => tasks.push(run(cx)) };
  return cx;
}

test("starting a new game releases an idle uncertain request", () => {
  const forgotten = [];
  const scope = context({ forget: id => forgotten.push(id) });
  const view = new scope.Game();
  view.motion = new Motion();
  Object.assign(view, { available: true, loading: false, busy: false,
    pending: "uncertain-request", game: game.newGame(2) });
  view.start(host(), 1);
  assert.deepEqual(forgotten, ["uncertain-request"]);
  assert.equal(view.pending, null);
  assert.equal(view.game.human, 1);
  assert.equal(view.game.turn, 1);
});

test("a late model result cannot change a restarted game", async () => {
  let reply;
  const forgotten = [], tasks = [];
  const scope = context({ prepare: () => "old-turn",
    execute: () => new Promise(resolve => reply = resolve),
    forget: id => forgotten.push(id) });
  const view = new scope.Game();
  view.motion = new Motion();
  Object.assign(view, { available: true, loading: false, busy: false, pending: null,
    model: { kind: "provider", model: "test/model" }, game: game.newGame(2), error: null });
  view.motion.reset(view.game);
  const cx = host(tasks);
  view.advance(cx);
  assert.equal(view.busy, true);
  view.start(cx, 1);
  const fresh = view.game;
  reply(JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } }));
  await Promise.all(tasks);
  assert.equal(view.game, fresh);
  assert.equal(view.game.moves.length, 0);
  assert.equal(view.pending, null);
  assert.equal(view.error, null);
  assert.deepEqual(forgotten, ["old-turn"]);
});

test("a human move triggers one model move and returns the turn", async () => {
  const sent = [], forgotten = [], tasks = [];
  const scope = context({
    prepare: command => { sent.push(JSON.parse(command)); return "model-turn"; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } }),
    forget: id => forgotten.push(id),
  });
  const view = new scope.Game();
  view.motion = new Motion();
  Object.assign(view, { available: true, loading: false, busy: false, pending: null,
    model: { kind: "provider", model: "test/model" }, game: game.newGame(1), error: null });
  view.motion.reset(view.game);
  const cx = host(tasks);
  view.act(7, 7, cx);
  await Promise.all(tasks);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].kind, "generate_plugin_text");
  assert.equal(sent[0].data.model, "test/model");
  assert.equal(JSON.parse(sent[0].data.prompt.split("\n").at(-1)).state.board[7][7], "X");
  assert.equal(view.game.moves.length, 2);
  assert.equal(view.game.board[7][7], 1);
  assert.equal(view.game.turn, 1);
  assert.equal(view.busy, false);
  assert.equal(view.error, null);
  assert.deepEqual(forgotten, ["model-turn"]);
});

test("the configured provider model completes an opening turn through the durable command", async () => {
  const sent = [], forgotten = [], tasks = [];
  const scope = context({
    prepare: command => { sent.push(JSON.parse(command)); return "opening-turn"; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 1 } } }),
    forget: id => forgotten.push(id),
  });
  const instance = new scope.Game();
  Object.assign(instance, { available: true, loading: false, busy: false, pending: null,
    model: { kind: "provider", model: "test/model" },
    game: game.newGame(2), motion: new Motion(), error: null, wins: 0, losses: 0, draws: 0 });
  instance.motion.reset(instance.game);
  instance.advance(host(tasks));
  await Promise.all(tasks);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].kind, "generate_plugin_text");
  assert.equal(sent[0].data.model, "test/model");
  assert.ok(JSON.parse(sent[0].data.prompt.split("\n").at(-1)).choices.length > 0);
  assert.equal(instance.game.moves.length, 1);
  assert.equal(instance.game.turn, instance.game.human);
  assert.equal(instance.busy, false);
  assert.equal(instance.error, null);
  assert.deepEqual(forgotten, ["opening-turn"]);
});
