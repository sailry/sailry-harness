import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import * as game from "../dev.sailry.platform/desktop/game.js";
import * as model from "../../tests/model.mjs";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default class", "globalThis.LiarsDice = class");

function context(tasks) {
  const cx = { notify() {}, sleep: async () => {},
    timer: { every: () => ({ cancel() {} }) },
    spawn: callback => { tasks.push(callback(cx)); } };
  return cx;
}

test("disabled opponents receive no dice or model requests", async () => {
  const sent = [], tasks = [];
  const scope = { ...game, ...model, View: class {},
    prepare: value => { sent.push(JSON.parse(value)); return "request"; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 3 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.LiarsDice();
  Object.assign(view, { pending: null, busy: false, available: true, loading: false,
    tokens: 0, enabled: [true, false, true], players: [null, { model: "second/model", kind: "provider" }] });
  const cx = context(tasks);
  view.start(cx);
  assert.deepEqual(view.game.counts, [5, 0, 5]);
  view.face = view.game.hands[0][0];
  view.act("bid", cx);
  while (tasks.length) await tasks.shift();
  assert.equal(view.error, null);
  assert.deepEqual(sent.map(request => request.data.model), ["second/model"]);
  assert.deepEqual(view.game.counts, [5, 0, 4]);
  view.next(cx);
  while (tasks.length) await tasks.shift();
  assert.equal(view.game.hands[1].length, 0);
  assert(sent.every(request => request.data.model === "second/model"));
});

test("consecutive AI seats use separate models and settle one challenge", async () => {
  const sent = [], tasks = [];
  const scope = { ...game, ...model, View: class {},
    prepare: value => { sent.push(JSON.parse(value)); return `request-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 3 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.LiarsDice();
  const match = game.deal(() => 0, [5, 5, 5], 1);
  Object.assign(view, { game: match, pending: null, busy: false, error: null, tokens: 0,
    roundPlayers: [{ model: "first/model", kind: "provider" },
      { model: "second/model", kind: "provider" }] });
  view.advance(context(tasks));
  while (tasks.length) await tasks.shift();
  assert.deepEqual(sent.map(request => request.data.model), ["first/model", "second/model"]);
  assert.equal(match.phase, "round_over");
  assert.deepEqual(match.counts, [5, 5, 4]);
  assert.deepEqual(match.history.map(entry => entry.kind), ["bid", "challenge"]);
  assert.equal(view.tokens, 6);
  assert.equal(view.error, null);
  const first = JSON.parse(sent[0].data.prompt.split("\n").at(-1));
  const second = JSON.parse(sent[1].data.prompt.split("\n").at(-1));
  assert.deepEqual(first.state.hand, match.hands[1]);
  assert.deepEqual(second.state.hand, match.hands[2]);
  assert(!("hands" in first.state));
  assert(!("hands" in second.state));
});

test("provider seats use their own concealed dice and reject an invalid reply", async () => {
  const sent = [], tasks = [], notices = [];
  let attempts = 0;
  const scope = { ...game, ...model, View: class {},
    toast: value => notices.push(value),
    prepare: value => { sent.push(JSON.parse(value)); return `request-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text",
      data: { text: JSON.stringify({ move: attempts++ === 0 ? "01" : 0 }), tokens: 2 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.LiarsDice();
  const match = game.deal(() => 0, [5, 5, 5], 1);
  Object.assign(view, { game: match, pending: null, busy: false, error: null, tokens: 0, text: messages("en"),
    roundPlayers: [{ model: "first/dice", kind: "provider" },
      { model: "second/dice", kind: "provider" }] });
  const cx = context(tasks);
  view.advance(cx);
  while (tasks.length) await tasks.shift();
  assert.equal(view.error, "invalidResponse");
  assert.equal(notices.length, 1);
  assert.equal(notices[0].message, view.text.invalidResponse);
  assert.equal(match.turn, 1);
  assert.equal(match.history.length, 0);
  view.advance(cx);
  while (tasks.length) await tasks.shift();
  assert.equal(view.error, null);
  assert.equal(notices.length, 1);
  assert.equal(view.tokens, 4);
  assert.equal(match.phase, "round_over");
  assert.deepEqual(match.counts, [5, 5, 4]);
  assert.deepEqual(sent.map(request => request.data.model),
    ["first/dice", "first/dice", "second/dice"]);
  const turns = sent.map(request => JSON.parse(request.data.prompt.split("\n").at(-1)));
  assert(sent.every(request => request.kind === "generate_plugin_text" && !("effort" in request.data)));
  assert(turns.every(turn => !("hands" in turn.state)));
  assert.deepEqual(turns[0].state.hand, match.hands[1]);
  assert.deepEqual(turns[2].state.hand, match.hands[2]);
  assert.deepEqual(turns[0].choices[0], { id: 0, kind: "bid", quantity: 1, face: 1 });
  assert.deepEqual(turns[2].choices[0], { id: 0, kind: "challenge" });
});

test("reset ignores and forgets a late model response", async () => {
  let reply, request, forgotten = 0;
  const tasks = [];
  const scope = { ...game, ...model, View: class {},
    prepare: () => "old-turn",
    execute: id => { request = id; return new Promise(resolve => reply = resolve); },
    forget: id => { assert.equal(id, "old-turn"); forgotten++; },
    outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.LiarsDice();
  const players = [{ model: "first/model", kind: "provider" },
    { model: "second/model", kind: "provider" }];
  Object.assign(view, { game: game.deal(() => 0, [5, 5, 5], 1), pending: null,
    busy: false, error: null, tokens: 0, available: true, loading: false, players,
    roundPlayers: players, enabled: [true, true, true], names: ["You", "First", "Second"] });
  const cx = context(tasks);
  view.advance(cx);
  assert.equal(request, "old-turn");
  view.reset(cx);
  const fresh = view.game;
  assert.equal(fresh.turn, 0);
  reply(JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 3 } } }));
  while (tasks.length) await tasks.shift();
  assert.equal(view.game, fresh);
  assert.equal(fresh.history.length, 0);
  assert.equal(view.tokens, 0);
  assert.equal(view.pending, null);
  assert.equal(forgotten, 1);
});

test("an admitted model request is observed by the same ID", async () => {
  let calls = 0, polls = 0;
  const expected = { Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 1 } } };
  const result = await model.completion("original", async id => {
    assert.equal(id, "original"); calls++;
    return JSON.stringify({ Err: { code: "outcome_unknown" } });
  }, async id => {
    assert.equal(id, "original");
    return JSON.stringify(++polls === 3
      ? { Ok: { kind: "completed", data: expected } }
      : { Ok: { kind: "admitted" } });
  }, async ms => assert.equal(ms, 1000));
  assert.deepEqual(result, expected);
  assert.equal(calls, 1);
  assert.equal(polls, 3);
});
