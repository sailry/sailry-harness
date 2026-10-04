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

function context(overrides = {}) {
  const scope = { ...game, ...model, Motion, View: class {},
    toast() {},
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

function view(scope, human = 1) {
  const instance = new scope.Game();
  Object.assign(instance, { available: true, loading: false, busy: false, pending: null,
    model: { kind: "provider", model: "test/model" }, game: game.newGame(human), error: null,
    wins: 0, losses: 0, draws: 0, motion: new Motion(), text: messages("en") });
  instance.motion.reset(instance.game);
  return instance;
}

test("restarting releases an idle uncertain request", () => {
  const forgotten = [];
  const scope = context({ forget: id => forgotten.push(id) });
  const instance = view(scope, 2);
  instance.pending = "uncertain-request";
  instance.start(host(), 1);
  assert.deepEqual(forgotten, ["uncertain-request"]);
  assert.equal(instance.pending, null);
  assert.equal(instance.game.human, 1);
  assert.equal(instance.game.turn, 1);
});

test("late AI results cannot alter a restarted game", async () => {
  let reply;
  const forgotten = [], tasks = [];
  const scope = context({ prepare: () => "old-turn",
    execute: () => new Promise(resolve => reply = resolve),
    forget: id => forgotten.push(id) });
  const instance = view(scope, 2);
  const cx = host(tasks);
  instance.advance(cx);
  assert.equal(instance.busy, true);
  instance.start(cx, 1);
  const fresh = instance.game;
  reply(JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } }));
  await Promise.all(tasks);
  assert.equal(instance.game, fresh);
  assert.equal(instance.game.history.length, 0);
  assert.equal(instance.pending, null);
  assert.equal(instance.error, null);
  assert.deepEqual(forgotten, ["old-turn"]);
});

test("a human opening creates exactly one legal AI response", async () => {
  const sent = [], forgotten = [], tasks = [];
  const scope = context({
    prepare: command => { sent.push(JSON.parse(command)); return "model-turn"; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } }),
    forget: id => forgotten.push(id),
  });
  const instance = view(scope);
  instance.act(2, 3, host(tasks));
  await Promise.all(tasks);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].kind, "generate_plugin_text");
  assert.equal(sent[0].data.model, "test/model");
  assert.equal(JSON.parse(sent[0].data.prompt.split("\n").at(-1)).state.turn, "O");
  assert.equal(instance.game.history.filter(entry => !entry.pass).length, 2);
  assert.equal(instance.game.turn, 1);
  assert.equal(instance.busy, false);
  assert.equal(instance.error, null);
  assert.deepEqual(forgotten, ["model-turn"]);
});

test("the AI plays twice when the human has a forced pass", async () => {
  const sent = [], forgotten = [], tasks = [];
  const scope = context({
    prepare: command => { sent.push(JSON.parse(command)); return `turn-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } }),
    forget: id => forgotten.push(id),
  });
  const instance = view(scope, 2);
  instance.game.board = Array.from({ length: 8 }, () => Array(8).fill(1));
  instance.game.board[0][0] = instance.game.board[7][7] = 0;
  instance.game.board[0][1] = instance.game.board[7][6] = 2;
  instance.advance(host(tasks));
  await Promise.all(tasks);
  assert.equal(sent.length, 2);
  assert.equal(sent[0].data.model, "test/model");
  assert.equal(sent[1].data.model, "test/model");
  assert.equal(instance.game.history.filter(entry => entry.pass && entry.player === 2).length, 2);
  assert.equal(instance.game.phase, "over");
  assert.equal(instance.game.winner, 1);
  assert.equal(instance.losses, 1);
  assert.equal(instance.busy, false);
  assert.deepEqual(forgotten, ["turn-1", "turn-2"]);
});

test("retry observes the same uncertain AI request", async () => {
  const sent = [], requests = [], tasks = [];
  const scope = context({
    prepare: command => { sent.push(JSON.parse(command)); return "stable-turn"; },
    execute: async id => {
      requests.push(id);
      return requests.length === 1 ? JSON.stringify({ Err: { code: "unavailable" } })
        : JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}' } } });
    },
    outcome: async () => JSON.stringify({ Ok: { kind: "unknown" } }),
  });
  const instance = view(scope);
  const cx = host(tasks);
  instance.act(2, 3, cx);
  await Promise.all(tasks);
  assert.equal(instance.error, "unconfirmed");
  assert.equal(instance.pending, "stable-turn");
  instance.advance(cx);
  await Promise.all(tasks);
  assert.equal(instance.error, null);
  assert.equal(instance.pending, null);
  assert.equal(instance.game.turn, 1);
  assert.equal(sent.length, 1);
  assert.deepEqual(requests, ["stable-turn", "stable-turn"]);
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
  assert.equal(instance.game.history.filter(entry => !entry.pass).length, 1);
  assert.equal(instance.game.turn, instance.game.human);
  assert.equal(instance.busy, false);
  assert.equal(instance.error, null);
  assert.deepEqual(forgotten, ["opening-turn"]);
});
