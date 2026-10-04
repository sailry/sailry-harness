import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import * as game from "../dev.sailry.platform/desktop/game.js";
import * as model from "../../tests/model.mjs";
import { validCharacters } from "../dev.sailry.platform/desktop/art.js";
import { presentEvents } from "../dev.sailry.platform/desktop/motion.js";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

const source = readFileSync(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace("export default class", "globalThis.CityTrader = class");

function harness(options = {}) {
  const sent = [], forgotten = [], tasks = [], notices = [];
  const overlays = {};
  const scope = { ...game, ...model, View: class {}, validCharacters, presentEvents,
    toast: value => notices.push(value),
    decisionTile: view => view.game.auction?.tile ?? view.game.trade?.tile
      ?? view.game.debt?.tile ?? view.game.players[view.game.active].position,
    newGame: () => game.newGame(() => 0),
    applyAction: (match, action) => game.act(match, action, () => 0),
    prepare: value => {
      const request = JSON.parse(value);
      sent.push(request);
      return options.prepare?.(request, sent.length) || `request-${sent.length}`;
    },
    execute: id => options.execute?.(id, sent.at(-1))
      || JSON.stringify({ Ok: { kind: "plugin_text", data: { text: '{"move":0}', tokens: 2 } } }),
    setValue: (key, value, revision) => {
      const request = {kind: "write_plugin_value", data: {key, value: JSON.parse(JSON.stringify(value)), expected_revision: revision}};
      sent.push(request);
      return `request-${sent.length}`;
    },
    getValue: key => options.getValue?.(key),
    outcome: id => options.outcome?.(id) || assert.fail("confirmed result"),
    forget: id => { forgotten.push(id); options.forget?.(id); },
  };
  scope.completeRequest = id => model.completion(id, scope.execute, scope.outcome, async () => {});
  scope.prepareModel = (player, turn) => scope.prepare(JSON.stringify(model.command(player, turn)));
  vm.runInNewContext(source, scope);
  const view = new scope.CityTrader();
  const players = [{ model: "provider/first", kind: "provider", effort: "high" },
    { model: "provider/second", kind: "provider", effort: "none" }];
  const motion = options.motion || { moving: false, reset() {}, update() {}, async settle() {} };
  Object.assign(view, { game: game.newGame(() => 0), motion, players,
    roundPlayers: players.map(player => ({ ...player })), names: ["You", "First", "Second"],
    characters: [0, 1, 2], editingCharacter: 0, expanded: [0], dialogId: 0, dialogOpen: false,
    enabled: [true, true, true], available: true, loading: false, pending: null, busy: false, animating: false,
    thinking: false, error: null, elapsed: 0, tokens: 0, text: messages("en") });
  Object.defineProperties(overlays, { open: { get: () => view.dialogOpen }, count: { get: () => view.dialogId } });
  const cx = { notify() { options.notify?.(view); }, sleep: options.sleep || (async () => {}),
    timer: { every: () => ({ cancel() {} }) },
    spawn: callback => { tasks.push(callback(cx)); } };
  const drain = async () => { while (tasks.length) await tasks.shift(); };
  return { view, cx, sent, forgotten, tasks, drain, overlays, notices };
}

function tradeTurn(match) {
  match.phase = "trade";
  match.active = 0;
  match.turn = 1;
  match.players[0].cash = 500;
  match.players[1].cash = 500;
  match.properties[1].owner = 1;
  match.trade = { tile: 1, to: 1, price: 100, beforeRoll: true };
}

const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

test("tile feedback follows movement, reports settled cash once, and closes after two seconds", async () => {
  let finish;
  const pauses = [];
  let first = true;
  const motion = { moving: true, update() {}, settle: () => {
    if (!first) return Promise.resolve();
    first = false;
    return new Promise(resolve => { finish = resolve; });
  } };
  const { view, cx, drain } = harness({ motion, sleep: duration => new Promise(resolve => pauses.push({ duration, resolve })) });
  view.game.players[0].position = 2;
  view.game.players[1].position = 1;
  view.game.players[2].position = 3;
  const cash = view.game.players[0].cash;
  view.act({ kind: "roll" }, cx);
  assert.equal(view.dialogOpen, false);
  finish();
  await flush();
  assert.equal(view.dialogOpen, true);
  assert.equal(view.dialogKind, "event");
  assert.equal(view.feedback.amount, view.game.players[0].cash - cash);
  assert.equal(view.feedback.entry.key, "event");
  assert.equal(view.feedback.visible, false);
  assert.equal(pauses[0].duration, 24);
  pauses.shift().resolve();
  await flush();
  assert.equal(view.feedback.visible, true);
  assert.equal(pauses[0].duration, 2000);
  const checkpoint = JSON.stringify(view.game);
  view.act({ kind: "end" }, cx);
  assert.equal(JSON.stringify(view.game), checkpoint);
  pauses.shift().resolve();
  await drain();
  assert.equal(view.dialogOpen, false);
  assert.equal(view.animating, false);
  assert.equal(view.game.history.filter(entry => entry.key === "event" && entry.seat === 0).length, 1);
  assert.equal(view.game.history.filter(entry => entry.key === "end" && entry.seat === 0).length, 1);
  assert.equal(view.game.turn, 0);
  assert.equal(view.game.round, 2);
});

test("dismissed feedback never closes a newer property dialog", async () => {
  const pauses = [];
  const { view, cx, drain } = harness({ sleep: duration => new Promise(resolve => pauses.push({ duration, resolve })) });
  const presentation = presentEvents(view, view.game, [{ key: "bonus", seat: 0, amount: 150 }], cx);
  await flush();
  pauses.shift().resolve();
  await flush();
  view.inspect(1, cx);
  const id = view.dialogId;
  await flush();
  pauses.shift().resolve();
  await presentation;
  await drain();
  assert.equal(view.dialogOpen, true);
  assert.equal(view.dialogKind, "property");
  assert.equal(view.dialogId, id);
});

test("unpaid tax is marked due and transitions to a persistent debt decision", async () => {
  const observed = [];
  const { view, cx, drain } = harness({ notify: view => {
    if (view.feedback) observed.push({ amount: view.feedback.amount, due: view.feedback.due });
  } });
  view.game.players[0].position = 6;
  view.game.players[0].cash = 10;
  view.act({ kind: "roll" }, cx);
  await drain();
  assert(observed.length > 0);
  assert(observed.every(item => item.due && item.amount === -100));
  assert.equal(view.game.players[0].cash, 10);
  assert.equal(view.game.phase, "debt");
  assert.equal(view.dialogKind, "property");
  assert.equal(view.dialogOpen, true);
});

test("AI tile feedback pauses its next decision until dismissed", async () => {
  const pauses = [];
  const { view, cx, sent, drain } = harness({
    sleep: duration => new Promise(resolve => pauses.push({ duration, resolve })),
  });
  view.game.turn = view.game.active = 1;
  view.game.players[1].position = 2;
  view.game.players[2].position = 1;
  view.advance(cx);
  await flush();
  assert.equal(view.dialogKind, "event");
  assert.equal(view.feedback.entry.seat, 1);
  assert.equal(view.game.turn, 1);
  assert.equal(sent.length, 0);
  view.closeDetails(cx);
  pauses.shift().resolve();
  await drain();
  assert.equal(view.game.turn, 0);
  assert.equal(view.dialogOpen, false);
});

test("a human turn and both AI seats advance without asking a model to roll", async () => {
  const { view, cx, sent, drain } = harness();
  view.game.players[1].position = 1;
  view.game.players[2].position = 3;
  view.act({ kind: "roll" }, cx);
  await drain();
  assert.equal(view.game.phase, "buy");
  assert.equal(view.game.players[0].position, 2);
  view.act({ kind: "buy" }, cx);
  await drain();

  assert.equal(view.error, null);
  assert.equal(view.busy, false);
  assert.equal(view.game.phase, "roll");
  assert.equal(view.game.turn, 0);
  assert.equal(view.game.properties[2].owner, 0);
  assert.equal(view.game.properties[3].owner, 1);
  assert.equal(view.game.properties[5].owner, 2);
  assert.deepEqual(sent.map(request => request.kind),
    ["generate_plugin_text", "generate_plugin_text",
      "generate_plugin_text", "generate_plugin_text"]);
  assert.deepEqual(sent.map(request => request.data.model),
    ["provider/first", "provider/first",
      "provider/second", "provider/second"]);
  assert.deepEqual(sent.map(request => JSON.parse(request.data.prompt.split("\n").at(-1)).state.phase),
  ["buy", "manage", "buy", "manage"]);
  assert.deepEqual(sent.map(request => request.data.effort), ["high", "high", "none", "none"]);
  assert.equal(view.tokens, 8);
  assert.equal(view.game.history.filter(entry => entry.key === "roll").length, 3);
});

test("an invalid model choice leaves the turn unchanged and can be retried", async () => {
  let attempts = 0;
  const { view, cx, sent, drain } = harness({ execute: () => JSON.stringify({ Ok: {
    kind: "plugin_text", data: { text: attempts++ === 0 ? '{"move":99}' : '{"move":0}', tokens: 1 },
  } }) });
  tradeTurn(view.game);
  view.advance(cx);
  await drain();
  assert.equal(view.error, "invalidResponse");
  assert.equal(view.game.turn, 1);
  assert.equal(view.game.phase, "trade");
  assert.equal(view.game.properties[1].owner, 1);
  assert.equal(view.pending, null);
  view.advance(cx);
  await drain();
  assert.equal(view.error, null);
  assert.equal(view.game.turn, 0);
  assert.equal(view.game.properties[1].owner, 0);
  assert.equal(view.game.players[0].cash, 400);
  assert.equal(view.game.players[1].cash, 600);
  assert.equal(sent.length, 2);
});

test("a durable provider result resumes with the original request ID", async () => {
  const dispatched = [], polled = [];
  const { view, cx, sent, forgotten, drain } = harness({
    execute: id => {
      dispatched.push(id);
      return JSON.stringify({ Err: { code: "outcome_unknown" } });
    },
    outcome: id => {
      polled.push(id);
      return JSON.stringify(polled.length === 2
        ? { Ok: { kind: "completed", data: { Ok: { kind: "plugin_text",
          data: { text: '{"move":0}', tokens: 2 } } } } }
        : { Ok: { kind: "admitted" } });
    },
  });
  tradeTurn(view.game);
  view.roundPlayers[0] = { model: "provider/first", kind: "provider" };
  view.advance(cx);
  await drain();
  assert.equal(view.error, null);
  assert.equal(view.game.properties[1].owner, 0);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].kind, "generate_plugin_text");
  assert.deepEqual(dispatched, ["request-1"]);
  assert.deepEqual(polled, ["request-1", "request-1"]);
  assert.deepEqual(forgotten, ["request-1"]);
});

test("reset discards a late model response and keeps the new match", async () => {
  let reply;
  const { view, cx, forgotten, tasks } = harness({
    execute: () => new Promise(resolve => { reply = resolve; }),
  });
  tradeTurn(view.game);
  view.advance(cx);
  const old = view.game;
  assert.equal(view.pending, "request-1");
  view.reset(cx);
  const fresh = view.game;
  assert.notEqual(fresh, old);
  assert.equal(fresh.phase, "roll");
  assert.equal(fresh.turn, 0);
  reply(JSON.stringify({ Ok: { kind: "plugin_text",
    data: { text: '{"move":0}', tokens: 5 } } }));
  while (tasks.length) await tasks.shift();
  assert.equal(view.game, fresh);
  assert.equal(fresh.history.length, 0);
  assert.equal(view.pending, null);
  assert.equal(view.tokens, 0);
  assert.deepEqual(forgotten, ["request-1"]);
});

test("a moving roll blocks a second human action until it settles", async () => {
  const releases = [];
  const motion = { moving: false, reset() {},
    update() { this.moving = true; },
    settle: () => new Promise(resolve => releases.push(resolve)) };
  const { view, cx, tasks } = harness({ motion,
    prepare: () => assert.fail("a human roll does not call a model") });
  view.act({ kind: "roll" }, cx);
  assert.equal(view.animating, true);
  assert.equal(view.game.phase, "buy");
  const history = view.game.history.length;
  view.act({ kind: "buy" }, cx);
  assert.equal(view.game.history.length, history);
  assert.equal(view.game.properties[2].owner, null);
  releases.shift()();
  while (tasks.length) await tasks.shift();
  assert.equal(view.animating, false);
  motion.update = () => { motion.moving = false; };
  motion.settle = async () => {};
  view.act({ kind: "buy" }, cx);
  assert.equal(view.game.properties[2].owner, 0);
  while (tasks.length) await tasks.shift();
  assert.equal(view.game.history.filter(entry => entry.key === "roll" && entry.seat === 0).length, 1);
});


test("save locks the checkpoint until receipt and resumes its owned snapshot", async () => {
  let reply;
  const {view,cx,sent,forgotten,drain,notices}=harness({execute:()=>new Promise(resolve=>{reply=resolve;})});
  view.saveGame(cx);
  assert.equal(view.saving,true);
  assert.equal(notices.length,0);
  const original=view.game;
  view.act({kind:"roll"},cx);
  view.reset(cx);
  assert.equal(view.game,original);
  assert.equal(view.game.phase,"roll");
  assert.equal(sent[0].data.expected_revision,"0");
  const saved={key:"match",revision:"1",present:true,value:sent[0].data.value};
  reply(JSON.stringify({Ok:{kind:"plugin_value",data:saved}}));
  await drain();
  assert.equal(view.saving,false);
  assert.equal(view.saved.revision,"1");
  assert.equal(view.savedAtHistory,0);
  assert.equal(notices.length,1);
  assert.equal(notices[0].kind,"info");
  assert.equal(notices[0].message,view.text.saved);
  assert.deepEqual(forgotten,["request-1"]);
  game.act(view.game,{kind:"roll"},()=>0);
  view.resume(cx);
  assert.equal(view.game.phase,"roll");
  assert.notEqual(view.game,original);
  view.saved.value.names=null;
  const resumed=view.game;
  view.resume(cx);
  assert.equal(view.game,resumed);
  assert.equal(view.saveError,"invalidSave");
  assert.equal(notices.length,2);
  assert.equal(notices[1].message,view.text.invalidSave);
});

test("uncertain save retries its original ID without permitting a new game", async () => {
  let attempts=0;
  const {view,cx,sent,forgotten,drain,notices}=harness({
    execute:()=>JSON.stringify({Err:{code:"outcome_unknown"}}),
    outcome:()=>JSON.stringify(++attempts===1 ? {Ok:{kind:"unknown"}} :
      {Ok:{kind:"completed",data:{Ok:{kind:"plugin_value",data:{key:"match",revision:"1",present:true,value:{}}}}}})
  });
  view.saveGame(cx);await drain();
  assert.equal(view.saveError,"saveUnconfirmed");
  assert.equal(view.savePending,"request-1");
  assert.equal(notices.length,1);
  assert.equal(notices[0].message,view.text.saveUnconfirmed);
  const game=view.game;view.reset(cx);assert.equal(view.game,game);
  view.saveGame(cx);await drain();
  assert.equal(view.savePending,null);
  assert.equal(view.saveError,null);
  assert.equal(notices.length,2);
  assert.equal(notices[1].kind,"info");
  assert.equal(sent.length,1);
  assert.deepEqual(forgotten,["request-1"]);
});

test("save conflict preserves play and reloads revision before an explicit retry", async () => {
  const {view,cx,sent,drain}=harness({
    execute:()=>JSON.stringify({Err:{code:"revision_conflict"}}),
    outcome:()=>JSON.stringify({Ok:{kind:"completed",data:{Err:{code:"revision_conflict"}}}}),
    getValue:()=>({revision:"3",present:true,value:{}})
  });
  view.saveGame(cx);await drain();
  assert.equal(view.saveError,"saveConflict");
  assert.equal(view.savePending,null);
  const game=view.game;
  view.refreshSave(cx);await drain();
  assert.equal(view.game,game);
  assert.equal(sent.length,1);
  view.saveGame(cx);await drain();
  assert.equal(sent[1].data.expected_revision,"3");
});

test("human decisions open after movement and dismissal never spends cash", async () => {
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  const motion = { moving: true, update() {}, settle: () => pending };
  const { view, cx, drain, overlays } = harness({ motion });
  view.act({ kind: "roll" }, cx);
  assert.equal(overlays.open, false);
  const cash = view.game.players[0].cash;
  finish();
  await drain();
  assert.equal(overlays.open, true);
  assert.equal(overlays.count, 1);
  const history = view.game.history.length;
  view.closeDetails();
  view.advance(cx);
  assert.equal(overlays.open, false);
  assert.equal(view.game.phase, "buy");
  assert.equal(view.game.players[0].cash, cash);
  assert.equal(view.game.history.length, history);
  view.inspect(2, cx);
  assert.equal(overlays.open, true);
});

test("AI decisions preserve expanded property groups", async () => {
  const { view, cx, drain, overlays } = harness();
  view.expanded = [0, 2];
  view.game.turn = view.game.active = 1;
  view.advance(cx);
  await drain();
  assert.equal(view.game.turn, 0);
  assert.equal(overlays.count, 1);
  assert.equal(overlays.open, false);
  assert.equal(view.dialogKind, "event");
  assert.deepEqual(Array.from(view.expanded), [0, 2]);
});

test("character choices swap occupied seats and round-trip with the save", async () => {
  const { view, cx, drain, sent } = harness({ execute: (_id, request) => JSON.stringify({
    Ok: { data: { revision: "1", value: request.data.value } },
  }) });
  view.game = null;
  view.chooseCharacter(2, cx);
  assert.deepEqual(Array.from(view.characters), [2, 1, 0]);
  view.chooseCharacter(5, cx);
  view.start(cx);
  view.saveGame(cx);
  await drain();
  assert.deepEqual(sent[0].data.value.characters, [5, 1, 0]);
  view.characters = [0, 1, 2];
  view.resume(cx);
  assert.deepEqual(Array.from(view.characters), [5, 1, 0]);
  assert.equal(view.saveError, null);
});

test("invalid saved character choices leave the current game untouched", () => {
  const { view, cx } = harness();
  const original = view.game;
  view.saved = { value: { game: JSON.parse(JSON.stringify(original)), names: view.names, characters: [0, 0, 8] } };
  view.resume(cx);
  assert.equal(view.game, original);
  assert.equal(view.saveError, "invalidSave");
});

test("the final AI turn opens results once and preserves board inspection", async () => {
  const { view, cx, drain, overlays } = harness();
  view.game.round = view.game.limit;
  view.game.active = view.game.turn = 2;
  view.game.phase = "manage";
  view.advance(cx);
  await drain();
  assert.equal(view.game.phase, "over");
  assert.equal(view.dialogKind, "result");
  assert.equal(overlays.open, true);
  assert.equal(overlays.count, 1);
  const final = JSON.stringify(view.game);
  view.closeDetails(cx);
  view.advance(cx);
  assert.equal(overlays.open, false);
  view.inspect(2, cx);
  assert.equal(view.dialogKind, "property");
  view.closeDetails(cx);
  view.showResult(cx);
  assert.equal(view.dialogKind, "result");
  assert.equal(overlays.open, true);
  assert.equal(JSON.stringify(view.game), final);
  view.showLobby(cx);
  assert.equal(overlays.open, false);
  assert.equal(view.game, null);
});

test("resuming a completed match opens the result without another AI request", async () => {
  const { view, cx, drain, sent } = harness();
  view.game.round = view.game.limit;
  view.game.active = view.game.turn = 2;
  view.game.phase = "manage";
  view.advance(cx);
  await drain();
  view.saved = { value: { game: JSON.parse(JSON.stringify(view.game)), names: view.names, characters: view.characters } };
  view.showLobby(cx);
  view.resume(cx);
  assert.equal(view.game.phase, "over");
  assert.equal(view.dialogKind, "result");
  assert.equal(view.dialogOpen, true);
  assert.equal(sent.length, 0);
});
