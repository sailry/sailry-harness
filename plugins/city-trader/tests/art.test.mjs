import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { newGame } from "../dev.sailry.platform/desktop/game.js";
import { building, character, diceImage, diceShadow, currencies, townPattern, actionSheet } from "../dev.sailry.platform/desktop/art.js";

const root = new URL("../", import.meta.url);
const manifest = JSON.parse(readFileSync(new URL("plugin.json", root)));
const resources = new Set(manifest.extensions["dev.sailry.platform"].desktop.resources);

test("every board, character and animation frame is a declared transparent PNG", () => {
  const paths = [...newGame(() => 0).board.map(building), diceShadow];
  for (let index = 0; index < 6; index++) paths.push(character(index), character(index, true));
  for (let frame = 0; frame < 20; frame++) paths.push(diceImage(1, frame));
  for (let face = 1; face <= 6; face++) {
    paths.push(diceImage(face));
    for (let frame = 0; frame < 8; frame++) paths.push(diceImage(face, frame, true));
    assert.deepEqual(readFileSync(new URL(diceImage(face), root)),
      readFileSync(new URL(diceImage(face, 7, true), root)));
  }
  assert.equal(new Set(paths).size, 109);
  for (const path of paths) {
    assert.ok(resources.has(path), `Undeclared asset: ${path}`);
    const bytes = readFileSync(new URL(path, root));
    assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a", path);
    assert.equal(bytes[25], 6, `Expected RGBA: ${path}`);
    assert.ok(bytes.readUInt32BE(16) <= 384 && bytes.readUInt32BE(20) <= 384, path);
  }
});

test("interface artwork is packaged as square transparent assets", () => {
  for (const path of [townPattern, ...Object.values(currencies)]) {
    assert(resources.has(path), `Undeclared asset: ${path}`);
    const bytes = readFileSync(new URL(path, root));
    assert.equal(bytes.subarray(0, 8).toString("hex"), "89504e470d0a1a0a");
    assert.equal(bytes[25], 6, `Expected RGBA: ${path}`);
    assert.equal(bytes.readUInt32BE(16), bytes.readUInt32BE(20));
    assert.equal(bytes.readUInt32BE(16), path === townPattern ? 960 : path === currencies.cash ? 176 : 64);
  }
});

test("artwork stays within display sizes and the remote bundle budget", () => {
  let encodedBytes = 0;
  assert(resources.size <= 128);
  for (const path of [...resources].filter(path => path.endsWith(".png"))) {
    const bytes = readFileSync(new URL(path, root));
    encodedBytes += Math.ceil(bytes.length / 3) * 4;
    const limit = path === townPattern ? 960 : path === actionSheet ? 256 : path === currencies.cash ? 176 : path.includes("/interface/") ? 64
      : path.includes("/portraits/") ? 96 : path.includes("/bodies/") ? 280
      : path.includes("/dice/") ? 176 : path.endsWith("11-botanical.png") ? 260
      : /(?:01-canal-walk|17-marina)\.png$/.test(path) ? 220 : 208;
    assert(bytes.readUInt32BE(16) <= limit && bytes.readUInt32BE(20) <= limit, path);
  }
  assert(encodedBytes <= 8 * 1024 * 1024, `Image bundle exceeds the Link budget: ${encodedBytes}`);
});

test("activity atlas provides twelve retina icons in one small resource", () => {
  assert(resources.has(actionSheet));
  const bytes = readFileSync(new URL(actionSheet, root));
  assert.equal(bytes[25], 6);
  assert.equal(bytes.readUInt32BE(16), 256);
  assert.equal(bytes.readUInt32BE(20), 192);
  assert(bytes.length < 80 * 1024);
});
