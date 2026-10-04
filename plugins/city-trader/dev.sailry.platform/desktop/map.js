import { CATALOG, EVENTS } from "./data.js";

export const MAP_SIZE = 7;
export const TILE_COUNT = (MAP_SIZE - 1) * 4;

function random(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let value = Math.imul(state ^ state >>> 15, state | 1);
    value ^= value + Math.imul(value ^ value >>> 7, value | 61);
    return ((value ^ value >>> 14) >>> 0) / 4294967296;
  };
}

function shuffle(values, next) {
  for (let index = values.length - 1; index > 0; index--) {
    const other = Math.floor(next() * (index + 1));
    [values[index], values[other]] = [values[other], values[index]];
  }
  return values;
}

function point(index) {
  const side = MAP_SIZE - 1;
  if (index <= side) return { column: index, row: 0 };
  if (index <= side * 2) return { column: side, row: index - side };
  if (index <= side * 3) return { column: side * 3 - index, row: side };
  return { column: 0, row: TILE_COUNT - index };
}

export function createMap(seed) {
  if (!Number.isInteger(seed) || seed < 0 || seed > 0xffffffff) throw new Error("Invalid map seed");
  const properties = shuffle(CATALOG.filter(tile => tile.kind === "property"), random(seed));
  return CATALOG.map((tile, index) => ({
    ...(tile.kind === "property" ? properties.shift() : tile), index, ...point(index),
  }));
}

export function cityEvent(seed, draw) {
  return EVENTS[Math.floor(random((seed ^ Math.imul(draw + 1, 0x9e3779b9)) >>> 0)() * EVENTS.length)];
}
