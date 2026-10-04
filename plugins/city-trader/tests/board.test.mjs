import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { newGame } from "../dev.sailry.platform/desktop/game.js";
import { MAP_SIZE } from "../dev.sailry.platform/desktop/map.js";

const scope = { MAP_SIZE };
const source = readFileSync(new URL("../dev.sailry.platform/desktop/board.js", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
vm.runInNewContext(`${source}\nglobalThis.position = tokenPosition;`, scope);

test("all occupied edges and corners retain an inner margin without stacking players", () => {
  for (const cell of [88, 108]) for (const tile of newGame(() => 0).board) {
    for (const count of [1, 2, 3]) {
      const positions = Array.from({ length: count }, (_, index) =>
        scope.position(tile.column, tile.row, cell, index, count));
      for (const { x, y } of positions) {
        assert(x >= cell + 14 && x + 34 <= 6 * cell - 14);
        assert(y >= cell + 14 && y + 46 <= 6 * cell - 14);
      }
      for (let index = 1; index < count; index++) {
        const before = positions[index - 1], after = positions[index];
        assert(Math.abs(after.x - before.x) >= 34 || Math.abs(after.y - before.y) >= 44);
      }
    }
  }
});
