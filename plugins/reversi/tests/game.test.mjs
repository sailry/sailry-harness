import test from "node:test";
import assert from "node:assert/strict";
import { SIZE, newGame, flips, legalMoves, counts, play, choices, coordinate,
  observation, turn } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

function prompt(game, moves) {
  return command({ kind: "provider", model: "test/model" }, turn(game, moves)).data.prompt;
}

function decision(text, moves) {
  return select({ Ok: { kind: "plugin_text", data: { text } } }, { kind: "provider" }, moves).move;
}


test("standard opening has four discs and four legal black moves", () => {
  const game = newGame();
  assert.deepEqual(counts(game.board), { black: 2, white: 2, empty: 60 });
  assert.deepEqual(choices(game), [
    { row: 2, column: 3 }, { row: 3, column: 2 },
    { row: 4, column: 5 }, { row: 5, column: 4 },
  ]);
  assert(!play(game, 2, 2, 3));
  assert(!play(game, 1, 3, 3));
  assert(!play(game, 1, -1, 3));
  assert(play(game, 1, 2, 3));
  assert.equal(game.board[3][3], 1);
  assert.deepEqual(counts(game.board), { black: 4, white: 1, empty: 59 });
  assert.equal(game.turn, 2);
});

test("one move flips captured discs in all eight directions", () => {
  const game = newGame();
  game.board = Array.from({ length: SIZE }, () => Array(SIZE).fill(0));
  for (const rise of [-1, 0, 1]) for (const across of [-1, 0, 1]) {
    if (!rise && !across) continue;
    game.board[3 + rise][3 + across] = 2;
    game.board[3 + 2 * rise][3 + 2 * across] = 1;
  }
  assert.equal(flips(game.board, 1, 3, 3).length, 8);
  assert(play(game, 1, 3, 3));
  assert.deepEqual(counts(game.board), { black: 17, white: 0, empty: 47 });
  assert.equal(game.phase, "over");
  assert.equal(game.winner, 1);
  assert.equal(game.history[0].flipped, 8);
});

function nearEnd(human = 1) {
  const game = newGame(human);
  game.board = Array.from({ length: SIZE }, () => Array(SIZE).fill(1));
  game.board[0][0] = 0;
  game.board[0][1] = 2;
  game.board[7][6] = 2;
  game.board[7][7] = 0;
  return game;
}

test("no legal move is an automatic pass, including a human pass", () => {
  const game = nearEnd(2);
  assert(play(game, 1, 0, 0));
  assert.equal(game.phase, "play");
  assert.equal(game.turn, 1);
  assert.deepEqual(legalMoves(game.board, 2), []);
  assert.deepEqual(choices(game), [{ row: 7, column: 7 }]);
  assert.deepEqual(game.history.at(-1), { player: 2, pass: true });
  assert(play(game, 1, 7, 7));
  assert.equal(game.phase, "over");
  assert.equal(game.turn, null);
  assert.equal(game.winner, 1);
  assert(!play(game, 2, 4, 4));
});

test("the game ends when both players cannot move even with empty squares", () => {
  const game = nearEnd();
  game.board[7][6] = 1;
  assert(play(game, 1, 0, 0));
  assert.equal(game.phase, "over");
  assert.equal(counts(game.board).empty, 1);
  assert.equal(game.winner, 1);
});

test("equal final disc counts produce a draw", () => {
  const game = newGame();
  game.board = Array.from({ length: SIZE }, () => Array(SIZE).fill(2));
  game.board[0][0] = 0;
  game.board[0][2] = 1;
  game.board[1][0] = 1;
  game.board[1][1] = 1;
  let black = 3;
  for (let row = 0; row < SIZE && black < 30; row++) for (let column = 0; column < SIZE && black < 30; column++) {
    if (row < 2 && column < 3) continue;
    game.board[row][column] = 1;
    black++;
  }
  assert.deepEqual(counts(game.board), { black: 30, white: 33, empty: 1 });
  assert.equal(flips(game.board, 1, 0, 0).length, 1);
  assert(play(game, 1, 0, 0));
  assert.deepEqual(counts(game.board), { black: 32, white: 32, empty: 0 });
  assert.equal(game.winner, 0);
});

test("model can choose only an offered public-board move", () => {
  const game = newGame();
  assert.equal(coordinate(2, 3), "D3");
  const moves = choices(game);
  const offer = JSON.parse(prompt(game, moves).split("\n").at(-1));
  assert.equal(offer.state.turn, "X");
  assert.equal(offer.state.board[3], "...OX...");
  assert.equal(offer.state.counts.black, 2);
  assert.deepEqual(decision('```json\n{"move":0}\n```', moves), moves[0]);
  assert.equal(observation(game).last, null);
  for (const response of ['{"move":-1}', `{"move":${moves.length}}`, '{"move":"0"}', "D3"])
    assert.throws(() => decision(response, moves));
});

test("seeded complete games conserve squares and stop with neither side able to move", () => {
  for (let seed = 1; seed <= 30; seed++) {
    const game = newGame();
    let state = seed, turns = 0;
    while (game.phase === "play") {
      const moves = choices(game);
      assert(moves.length > 0);
      state = (1664525 * state + 1013904223) >>> 0;
      const chosen = moves[state % moves.length];
      assert(play(game, game.turn, chosen.row, chosen.column));
      const score = counts(game.board);
      assert.equal(score.black + score.white + score.empty, 64);
      assert(++turns <= 60);
    }
    assert.deepEqual(legalMoves(game.board, 1), []);
    assert.deepEqual(legalMoves(game.board, 2), []);
    const score = counts(game.board);
    assert.equal(game.winner, score.black === score.white ? 0 : score.black > score.white ? 1 : 2);
  }
});
