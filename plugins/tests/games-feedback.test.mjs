import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import vm from "node:vm";
import * as model from "./model.mjs";

const plugins = ["gomoku", "reversi", "xiangqi", "poker", "liars-dice", "doudizhu", "city-trader"];
const source = path => readFileSync(path, "utf8").replace(/^import .*;\n/gm, "");
const player = { kind: "provider", model: "test/model" };

class Element {
  constructor(id = null) {
    this.items = [];
    this.props = id === null ? {} : { id };
    return new Proxy(this, { get(target, key, receiver) {
      if (key in target) return typeof target[key] === "function" ? target[key].bind(receiver) : target[key];
      return (...values) => { target.props[key] = values[0]; return receiver; };
    } });
  }
  child(value) { if (value !== null && value !== undefined) this.items.push(value); return this; }
  children(values) { values.forEach(value => this.child(value)); return this; }
  static new(id, props = {}) { const item = new Element().id(id); Object.assign(item.props, props); return item; }
}

function flatten(root) {
  return root instanceof Element ? [root, ...root.items.flatMap(flatten)] : [root];
}

function match(name, game) {
  if (name === "gomoku" || name === "reversi") return game.newGame(2);
  if (name === "xiangqi") return game.newGame("black");
  if (name === "poker") return game.deal(() => 0.5, [1000, 1000], 1);
  if (name === "liars-dice") return game.deal(() => 0, [5, 5, 5], 1);
  if (name === "doudizhu") return game.deal(() => 0.5, 1);
  const value = game.newGame(() => 0);
  Object.assign(value, { phase: "trade", active: 0, turn: 1,
    trade: { tile: 1, price: 100, buyer: 0, seller: 1 } });
  value.properties[1].owner = 1;
  return value;
}

async function harness(name, options = {}) {
  const base = new URL(`../${name}/dev.sailry.platform/desktop/`, import.meta.url);
  const game = await import(new URL("game.js", base));
  const { messages } = await import(new URL("locales.js", base));
  const notices = [], tasks = [];
  const scope = { ...game, ...model, View: class {}, applyAction: game.act,
    toast: value => notices.push(value), forget() {},
    completion: options.completion || (async () => { throw new Error("unconfirmed"); }),
    completeRequest: options.completion || (async () => { throw new Error("unconfirmed"); }),
    readSettings: options.readSettings || (async () => { throw new Error("unavailable"); }),
  };
  vm.runInNewContext(source(new URL("main.js", base)).replace("export default class", "globalThis.Game = class"), scope);
  const view = new scope.Game();
  Object.assign(view, { text: messages("en"), game: match(name, game),
    model: player, player, players: [player, player], roundPlayer: player, roundPlayers: [player, player],
    names: ["You", "First", "Second"], name: "First", characters: [0, 1, 2], enabled: [true, true, true],
    pending: "stable-turn", busy: false, animating: false, thinking: false,
    error: null, loading: false, available: true, elapsed: 0, tokens: 0, wins: 0, losses: 0, draws: 0,
    score: 0, selected: [], expanded: [0], editingCharacter: 0, dialogId: 0, dialogOpen: false,
    motion: { rolling: false, moving: false, reset() {}, update() {}, move() {}, async settle() {} },
  });
  const cx = { notify() {}, sleep: async () => {}, timer: { every: () => ({ cancel() {} }) },
    spawn: run => tasks.push(run(cx)) };
  const drain = async () => { while (tasks.length) await tasks.shift(); };
  const leaf = () => new Element();
  const ui = { ...game, retryable: model.retryable, div: leaf, Button: Element, IconButton: Element,
    Toggle: Element, Spinner: Element, Tag: Element, ShimmerText: Element, Icon: Element,
    Header: Element, Image: Element, Modal: Element, theme: () => ({ colors: {} }),
    material: {}, portrait: () => "portrait", character: () => "character", building: () => "building",
    window: { viewport_size: () => ({ width: 1280, height: 900 }) },
    board: leaf, history: leaf, result: leaf, details: leaf, eventDetails: leaf,
    stage: content => content, MAP_SIZE: 5, CATALOG: Array(24).fill({}),
    seat: leaf, portfolio: leaf, assets: leaf, auction: leaf, activity: leaf, AUCTION_HEIGHT: 0, ASSETS_HEIGHT: 0,
    card: leaf, cards: leaf, back: leaf, backs: leaf, hole: leaf, hand: leaf,
    felt: (_view, _compact, actions) => new Element().child(actions),
    table: leaf, die: leaf, effects: () => [], fireworks: () => [], tween: value => value,
  };
  for (const file of ["controls.js", "lobby.js", "view.js"]) {
    const path = new URL(file, base);
    if (existsSync(path)) vm.runInNewContext(source(path).replaceAll("export function", "function"), ui);
  }
  if (name === "city-trader") ui.portfolio = view => new Element().child(ui.status(view));
  return { view, cx, notices, drain, render: () => flatten(ui.render(view, cx)) };
}

for (const name of plugins) test(`${name}: uncertain turns toast once and retain retry`, async () => {
  const { view, cx, notices, drain, render } = await harness(name);
  const checkpoint = JSON.stringify(view.game);
  view.advance(cx);
  await drain();
  assert.equal(view.error, "unconfirmed");
  assert.equal(view.pending, "stable-turn");
  assert.equal(JSON.stringify(view.game), checkpoint);
  assert.equal(view.busy, false);
  assert.equal(notices.length, 1);
  assert.equal(notices[0].kind, "error");
  assert.equal(notices[0].message, view.text.unconfirmed);
  const items = [...render(), ...render()];
  assert(!items.includes(view.text.unconfirmed));
  assert(items.some(item => item instanceof Element && item.props.id?.endsWith("-retry")));
  assert.equal(notices.length, 1);
});

for (const name of plugins) test(`${name}: settings failures toast without an inline notice`, async () => {
  const { view, cx, notices, drain, render } = await harness(name);
  view.game = null;
  view.configure(cx);
  await drain();
  assert.equal(view.error, "loadFailed");
  assert.equal(view.loading, false);
  assert.equal(notices.length, 1);
  assert.equal(notices[0].message, view.text.loadFailed);
  const items = render();
  assert(!items.includes(view.text.loadFailed));
  const retries = items.filter(item => item instanceof Element && item.props.id?.includes("retry"));
  assert.equal(retries.length, name === "doudizhu" ? 1 : 0);
  assert.equal(notices.length, 1);
  if (name === "doudizhu") {
    assert.equal(retries[0].props.id, "ddz-retry");
    retries[0].props.on_click({}, cx);
    await drain();
    assert.equal(view.loading, false);
    assert.equal(view.error, "loadFailed");
    assert.equal(notices.length, 2);
    assert.equal(notices[1].message, view.text.loadFailed);
    assert(!render().includes(view.text.loadFailed));
    assert.equal(notices.length, 2);
  }
});

for (const name of plugins) test(`${name}: stale turn failures do not toast`, async () => {
  let reject;
  const { view, cx, notices, drain } = await harness(name, {
    completion: () => new Promise((_resolve, fail) => { reject = fail; }),
  });
  view.advance(cx);
  const fresh = {};
  view.game = fresh;
  reject(new Error("failed"));
  await drain();
  assert.equal(view.game, fresh);
  assert.equal(view.error, null);
  assert.equal(notices.length, 0);
});

test("city-trader: save feedback stays in toasts and keeps revision recovery", async () => {
  const { view, render, notices } = await harness("city-trader");
  view.saveError = "saveConflict";
  view.savedAtHistory = view.game.history.length;
  const items = render();
  assert(!items.includes(view.text.saveConflict));
  assert(items.some(item => item instanceof Element && item.props.id === "city-refresh-save"));
  const header = items.find(item => item instanceof Element && item.props.id === "city-header");
  const actions = JSON.parse(header.props.content).actions;
  assert.equal(actions.find(action => action.id === "city-header-save").label, view.text.save);
  assert.equal(notices.length, 0);
});
