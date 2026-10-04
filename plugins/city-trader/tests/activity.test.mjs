import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { newGame } from "../dev.sailry.platform/desktop/game.js";
import { tileName } from "../dev.sailry.platform/desktop/art.js";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

const scope = { tileName };
const source = readFileSync(new URL("../dev.sailry.platform/desktop/portfolio.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
vm.runInNewContext(`${source}\nglobalThis.item = activityItem;`, scope);
const view = { game: newGame(() => 0), text: messages("en") };

test("rewards and tax show the location once and unsigned amounts with cash flow", () => {
  const reward = scope.item(view, { key: "bonus", tile: 12, amount: 150 });
  assert.equal(reward.title, tileName(view.text, view.game.board[12]));
  assert.equal(reward.description, "150");
  assert.equal(reward.icon, 1);
  assert.equal(reward.flow, 150);
  const tax = scope.item(view, { key: "tax", tile: 8, amount: 100 });
  assert.equal(tax.description, "100");
  assert.equal(tax.icon, 2);
  assert.equal(tax.flow, -100);
});

test("property actions share the property title with distinct icons and hover labels", () => {
  const items = ["buy", "trade_proposed", "trade_accepted", "trade_rejected"].map(key =>
    scope.item(view, { key, tile: 1, price: 140 }));
  assert(items.every(item => item.title === tileName(view.text, view.game.board[1])));
  assert.equal(new Set(items.map(item => item.icon)).size, 4);
  assert.equal(new Set(items.map(item => item.label)).size, 4);
  assert.deepEqual(items.map(item => item.flow), [-140, 0, 140, 0]);
  assert.deepEqual(items.map(item => item.description), ["140", "140", "140", "140"]);
});

test("dice and random events retain their outcome without repeating the action", () => {
  const roll = scope.item(view, { key: "roll", dice: [5, 3] });
  assert.equal(roll.description, "8 steps");
  assert.equal(roll.flow, 0);
  const event = scope.item(view, { key: "event", tile: 4, event: "event.repair", amount: -80 });
  assert.equal(event.title, view.text.cityEvents["event.repair"]);
  assert.equal(event.description, "80");
  assert.equal(event.icon, 2);
  assert.equal(event.flow, -80);
});

test("auction passes retain the auction icon and describe the outcome", () => {
  const pass = scope.item(view, { key: "pass", tile: 1 });
  assert.equal(pass.icon, 7);
  assert.equal(pass.label, view.text.phase.auction);
  assert.equal(pass.description, view.text.action.pass);
  assert.equal(pass.currency, false);
  const won = scope.item(view, { key: "auction_won", tile: 1, amount: 280 });
  assert.equal(won.icon, pass.icon);
  assert.equal(won.label, pass.label);
  assert.equal(won.description, "280");
  assert.equal(won.flow, -280);
});
