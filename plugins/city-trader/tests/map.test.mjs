import test from "node:test";
import assert from "node:assert/strict";
import { createMap, cityEvent, MAP_SIZE, TILE_COUNT } from "../dev.sailry.platform/desktop/map.js";
import { CATALOG } from "../dev.sailry.platform/desktop/data.js";
import { newGame, restore, act, observation } from "../dev.sailry.platform/desktop/game.js";
import { building, tileName } from "../dev.sailry.platform/desktop/art.js";
import { messages } from "../dev.sailry.platform/desktop/locales.js";

test("seeded properties occupy a closed 24-tile perimeter with an empty center", () => {
  const routes = new Set(), placements = new Set();
  for (const seed of [...Array.from({ length: 128 }, (_, i) => i), 0xffffffff]) {
    const board = createMap(seed);
    assert.equal(TILE_COUNT, 24);
    assert.equal(board.length, TILE_COUNT);
    assert.equal(new Set(board.map(tile => `${tile.column}:${tile.row}`)).size, TILE_COUNT);
    board.forEach((tile, index) => {
      assert.equal(tile.index, index);
      assert(tile.column >= 0 && tile.column < MAP_SIZE);
      assert(tile.row >= 0 && tile.row < MAP_SIZE);
      assert(tile.column === 0 || tile.column === MAP_SIZE - 1
        || tile.row === 0 || tile.row === MAP_SIZE - 1);
      const next = board[(index + 1) % board.length];
      assert.equal(Math.abs(tile.column - next.column) + Math.abs(tile.row - next.row), 1);
    });
    assert.equal(board[0].kind, "start");
    assert.equal(board[1].kind, "property");
    assert.equal(board[2].kind, "property");
    assert.deepEqual(board.filter(tile => tile.kind === "property").map(tile => tile.art).sort((a, b) => a - b),
      CATALOG.filter(tile => tile.kind === "property").map(tile => tile.art));
    for (const tile of board) {
      assert.equal(building(tile), building(CATALOG[tile.art]));
      assert.equal(tile.price, CATALOG[tile.art].price);
      for (const locale of ["en", "zh-CN"]) assert.equal(typeof tileName(messages(locale), tile), "string");
    }
    assert.deepEqual(createMap(seed), board);
    routes.add(JSON.stringify(board.map(({ column, row }) => [column, row])));
    placements.add(JSON.stringify(board.map(tile => tile.art)));
  }
  assert.equal(routes.size, 1, "all seeds preserve the perimeter layout");
  assert.deepEqual([0, 6, 12, 18].map(index => {
    const { column, row } = createMap(0)[index];
    return [column, row];
  }), [[0, 0], [6, 0], [6, 6], [0, 6]]);
  assert(placements.size > 100, "property placement must vary between seeds");
});

test("saved games preserve their map and event sequence independently of new games", () => {
  const original = newGame(() => 0.25);
  const event = original.board.find(tile => tile.kind === "event").index;
  original.players[0].position = event - 2;
  act(original, { kind: "roll" }, () => 0);
  const saved = JSON.parse(JSON.stringify(original));
  const resumed = restore(saved);
  const fresh = newGame(() => 0.75);
  assert.notDeepEqual(fresh.board, resumed.board);
  assert.deepEqual(observation(resumed).board, original.board);
  assert.equal(resumed.history.at(-1).event, cityEvent(original.seed, 0).key);
  for (const game of [original, resumed]) {
    game.phase = "roll";
    game.players[0].position = event - 2;
    act(game, { kind: "roll" }, () => 0);
  }
  assert.deepEqual(resumed, original);
  assert.equal(resumed.history.at(-1).event, cityEvent(original.seed, 1).key);
  assert.equal(saved.eventIndex, 1);
});

test("invalid seeds and altered saved maps are rejected without changing the input", () => {
  for (const seed of [-1, 0x100000000, 0.5, NaN, undefined]) {
    assert.throws(() => createMap(seed), /Invalid map seed/);
  }
  for (const change of [
    game => { delete game.version; },
    game => { game.seed = -1; },
    game => { game.board[2].price++; },
    game => { game.board[2].column = game.board[1].column; game.board[2].row = game.board[1].row; },
    game => { game.board.pop(); },
  ]) {
    const game = newGame(() => 0);
    change(game);
    const before = structuredClone(game);
    assert.throws(() => restore(game), /Invalid saved game/);
    assert.deepEqual(game, before);
  }
});
