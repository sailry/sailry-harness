import test from "node:test";
import assert from "node:assert/strict";
import { SIZE, newGame, play, winningLine, choices, coordinate, turn } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

function prompt(game, moves) {
  return command({ kind: "provider", model: "test/model" }, turn(game, moves)).data.prompt;
}

function decision(text, moves) {
  return select({ Ok: { kind: "plugin_text", data: { text } } }, { kind: "provider" }, moves).move;
}


test("turns reject occupied, foreign, and out-of-bounds moves", () => {
  const game = newGame(2);
  assert(!play(game, 2, 7, 7));
  assert(!play(game, 1, -1, 7));
  assert(!play(game, 1, 7.5, 7));
  assert(play(game, 1, 7, 7));
  assert(!play(game, 2, 7, 7));
  assert(!play(game, 1, 7, 8));
  assert(play(game, 2, 7, 8));
  assert.equal(game.turn, 1);
  assert.equal(game.moves.length, 2);
});

test("five or more stones win in every direction", () => {
  const directions = [[0, 1], [1, 0], [1, 1], [1, -1]];
  for (const [rise, across] of directions) {
    const game = newGame();
    const first = across < 0 ? 8 : 4;
    for (let index = 0; index < 5; index++) {
      assert(play(game, 1, 4 + rise * index, first + across * index));
      if (index < 4) assert(play(game, 2, 0, index * 2));
    }
    assert.equal(game.phase, "over");
    assert.equal(game.winner, 1);
    assert.equal(game.winning.length, 5);
    assert(!play(game, 2, 14, 14));
  }
  const game = newGame();
  for (let column = 3; column <= 8; column++) game.board[7][column] = 1;
  assert.equal(winningLine(game.board, 7, 6, 1).length, 6);
});

test("edges do not join across rows and a full board is a draw", () => {
  const game = newGame();
  for (let column = 11; column < 15; column++) game.board[0][column] = 1;
  game.board[1][0] = 1;
  assert.deepEqual(winningLine(game.board, 1, 0, 1), []);

  const draw = newGame();
  for (let row = 0; row < SIZE; row++) for (let column = 0; column < SIZE; column++) {
    if (row === 0 && column === 0) continue;
    const player = (row + 2 * column) % 4 < 2 ? 1 : 2;
    draw.board[row][column] = player;
    draw.moves.push({ player, row, column });
  }
  for (let row = 0; row < SIZE; row++) for (let column = 0; column < SIZE; column++) {
    const player = draw.board[row][column];
    if (player) assert.deepEqual(winningLine(draw.board, row, column, player), []);
  }
  assert(play(draw, 1, 0, 0));
  assert.equal(draw.phase, "over");
  assert.equal(draw.winner, 0);
});

test("model sees the board and can select only an offered empty intersection", () => {
  const game = newGame();
  assert.deepEqual(choices(game), [{ row: 7, column: 7 }]);
  assert.equal(coordinate(7, 7), "H8");
  assert(play(game, 1, 7, 7));
  const moves = choices(game);
  assert(moves.length > 1);
  assert(moves.every(({ row, column }) => game.board[row][column] === 0));
  const offer = JSON.parse(prompt(game, moves).split("\n").at(-1));
  assert.equal(offer.state.board[7][7], "X");
  assert.equal(offer.state.turn, "O");
  assert.equal(offer.state.last, "H8");
  assert.deepEqual(decision('```json\n{"move":0}\n```', moves), moves[0]);
  for (const response of ['{"move":-1}', `{"move":${moves.length}}`, '{"move":"0"}', "H8"])
    assert.throws(() => decision(response, moves));
});
