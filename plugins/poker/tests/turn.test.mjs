import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import * as game from "../dev.sailry.platform/desktop/game.js";
import * as model from "../../tests/model.mjs";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default class", "globalThis.Poker = class");

test("AI closes preflop and opens the flop before returning control", async () => {
  const sent = [], tasks = [];
  const scope = { ...game, ...model, View: class {},
    prepare: value => { sent.push(JSON.parse(value)); return `request-${sent.length}`; },
    execute: async () => JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 2 } } }),
    forget() {}, outcome: () => assert.fail("confirmed result"),
  };
  scope.completion = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  vm.runInNewContext(source, scope);
  const view = new scope.Poker();
  const match = game.deal(() => 0.5);
  assert(game.act(match, 0, { kind: "call" }));
  Object.assign(view, { game: match, pending: null, busy: false, error: null,
    tokens: 0, roundPlayer: { model: "test/model", kind: "provider" } });
  const cx = { notify() {}, sleep: async () => {},
    timer: { every: () => ({ cancel() {} }) },
    spawn: callback => { tasks.push(callback(cx)); } };
  view.advance(cx);
  while (tasks.length) await tasks.shift();
  assert.equal(sent.length, 2);
  assert.deepEqual(sent.map(request => request.data.model), ["test/model", "test/model"]);
  assert.equal(match.stage, "flop");
  assert.equal(match.turn, 0);
  assert.equal(view.busy, false);
  assert.equal(view.error, null);
  assert.equal(view.tokens, 4);
});

test("the provider receives only visible cards and can retry an invalid move", async () => {
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
  const view = new scope.Poker();
  const match = game.deal(() => 0.5, [1000, 1000], 1);
  const privateHand = game.observation(match).hand;
  Object.assign(view, { game: match, pending: null, busy: false, error: null,
    tokens: 0, roundPlayer: { model: "test/poker", kind: "provider" }, text: messages("en") });
  const cx = { notify() {}, sleep: async () => {},
    timer: { every: () => ({ cancel() {} }) },
    spawn: callback => { tasks.push(callback(cx)); } };
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
  assert.equal(match.turn, 0);
  assert.equal(view.tokens, 2);
  assert.equal(sent.length, 2);
  assert(sent.every(request => request.kind === "generate_plugin_text"));
  assert.equal(sent[0].data.model, "test/poker");
  const turn = JSON.parse(sent[0].data.prompt.split("\n").at(-1));
  assert.deepEqual(turn.state.hand, privateHand);
  assert.deepEqual(turn.state.board, []);
  assert(!("hands" in turn.state));
  assert(!("effort" in sent[0].data));
  assert.deepEqual(turn.choices[0], { id: 0, kind: "call" });
});

test("raise confirmation validates the amount and cannot act during the opponent turn", () => {
  const notices = [];
  const scope = { ...game, View: class {}, toast: value => notices.push(value) };
  vm.runInNewContext(source, scope);
  const view = new scope.Poker();
  let advances = 0;
  Object.assign(view, { game: game.deal(() => 0.5), busy: false, raiseOpen: true,
    raiseTo: 19, advance: () => advances++, text: messages("en") });
  const cx = { notify() {} };
  view.raise(cx);
  assert.equal(view.error, "invalid");
  assert.equal(notices.length, 1);
  assert.equal(notices[0].message, view.text.invalid);
  assert.equal(view.game.history.length, 0);
  assert.equal(view.raiseOpen, true);
  view.raiseTo = 30;
  view.raise(cx);
  assert.equal(view.error, null);
  assert.equal(view.game.history[0].to, 30);
  assert.equal(view.raiseOpen, false);
  assert.equal(advances, 1);
  const checkpoint = JSON.stringify(view.game);
  view.raiseTo = 1000;
  view.raise(cx);
  assert.equal(JSON.stringify(view.game), checkpoint);
  assert.equal(view.error, null);
  assert.equal(advances, 1);
});

test("new hands clear the raise dialog and give results a fresh dismissal identity", () => {
  const scope = { ...game, View: class {} };
  vm.runInNewContext(source, scope);
  const view = new scope.Poker();
  Object.assign(view, { game: game.deal(() => 0.5), available: true, busy: false,
    player: { model: "test/model" }, resultId: 4, resultDismissed: true, raiseOpen: true, advance() {} });
  const cx = { notify() {} };
  assert(game.act(view.game, 0, { kind: "fold" }));
  const stacks = [...view.game.stacks];
  view.next(cx);
  assert.equal(view.resultId, 5);
  assert.equal(view.resultDismissed, false);
  assert.equal(view.raiseOpen, false);
  assert.equal(view.game.handNumber, 2);
  assert.equal(view.game.history.length, 0);
  assert.deepEqual([...view.game.startStacks], stacks);
});


test("a dismissed raise dialog cannot submit a move", () => {
  const scope = { ...game, View: class {} };
  vm.runInNewContext(source, scope);
  const view = new scope.Poker();
  Object.assign(view, { game: game.deal(() => 0.5), busy: false, raiseOpen: false,
    raiseTo: 1000, advance: () => assert.fail("dismissed dialog") });
  view.raise({ notify() {} });
  assert.equal(view.game.history.length, 0);
  view.setAmount();
  assert.equal(view.raiseTo, 20);
  view.game = game.deal(() => 0.5, [15, 1985]);
  view.setAmount();
  assert.equal(view.raiseTo, 15);
});


test("thinking starts at zero before the request and updates until it completes", async () => {
  let now = 1000, tick, cancelled = false;
  const tasks = [], notifications = [];
  const scope = { ...game, ...model, View: class {}, Date: { now: () => now },
    prepare: () => "request", forget() {},
    completion: async () => ({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 0 } } }) };
  vm.runInNewContext(source, scope);
  const view = new scope.Poker();
  Object.assign(view, { game: game.deal(() => 0.5, [1000, 1000], 1), busy: false,
    elapsed: 17, tokens: 0, roundPlayer: { model: "test/model", kind: "provider" } });
  const cx = {
    notify: () => notifications.push({ busy: view.busy, elapsed: view.elapsed }),
    timer: { every: (_delay, callback) => { tick = callback; return { cancel: () => { cancelled = true; } }; } },
    spawn: callback => tasks.push(callback),
  };
  view.advance(cx);
  assert.deepEqual(notifications, [{ busy: true, elapsed: 0 }]);
  now = 3000;
  tick(cx);
  assert.deepEqual(notifications.at(-1), { busy: true, elapsed: 2 });
  await tasks.shift()(cx);
  assert.equal(view.game.turn, 0);
  assert.equal(view.busy, false);
  assert.equal(cancelled, true);
});


test("raise multipliers follow the current bet and the available stack", () => {
  const controls = readFileSync(new URL("../dev.sailry.platform/desktop/controls.js", import.meta.url), "utf8")
    .replace(/^import .*;\n/gm, "").replaceAll("export function", "function");
  const scope = { ...game };
  vm.runInNewContext(controls, scope);
  const match = game.deal(() => 0.5);
  const targets = () => Array.from(scope.raiseTargets(match), option => option.to);
  assert.deepEqual(targets(), [20, 30, 50, 100]);
  assert(game.act(match, 0, { kind: "call" }));
  assert(game.act(match, 1, { kind: "raise", to: 80 }));
  assert.deepEqual(targets(), [160, 240, 400, 800]);
  for (const option of scope.raiseTargets(match)) {
    assert(option.enabled);
    assert(game.act(structuredClone(match), 0, { kind: "raise", to: option.to }));
  }
  for (const [stack, enabled] of [[15, []], [50, [20, 30, 50]]]) {
    const limited = game.deal(() => 0.5, [stack, 2000 - stack]);
    assert.deepEqual(Array.from(scope.raiseTargets(limited)).filter(option => option.enabled).map(option => option.to), enabled);
  }
  assert(game.act(match, 0, { kind: "call" }));
  assert(game.act(match, 1, { kind: "check" }));
  assert.equal(match.currentBet, 0);
  assert.deepEqual(targets(), [20, 30, 50, 100]);
});
