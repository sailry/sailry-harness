import test from "node:test";
import assert from "node:assert/strict";
import { START_FEN, newGame, fromFen, toFen, pseudoMoves, legalMoves, inCheck,
  play, choices, observation, turn } from "../dev.sailry.platform/desktop/game.js";
import { command, select } from "../../tests/model.mjs";

function prompt(game, moves) {
  return command({ kind: "provider", model: "test/model" }, turn(game, moves)).data.prompt;
}

function decision(text, moves) {
  return select({ Ok: { kind: "plugin_text", data: { text } } }, { kind: "provider" }, moves).move;
}


const empty = () => Array.from({ length: 10 }, () => Array(9).fill("."));
function base() {
  const board = empty();
  board[0][4] = "k";
  board[9][4] = "K";
  board[5][4] = "P";
  return board;
}
const destinations = moves => moves.map(move => `${move.toRow},${move.toColumn}`).sort();
const has = (moves, row, column) => moves.some(move => move.toRow === row && move.toColumn === column);
const positioned = (board, turn = "red") => newGame("red", toFen(board, turn));

test("standard opening round trips through FEN and offers only legal indexed moves", () => {
  const game = newGame();
  assert.equal(toFen(game.board, game.turn), START_FEN);
  assert.deepEqual(fromFen(START_FEN).board, game.board);
  assert.equal(choices(game).length, 44);
  assert.deepEqual(destinations(legalMoves(game.board, "red", 6, 0)), ["5,0"]);
  assert.equal(observation(game).turn, "red");
  const offer = JSON.parse(prompt(game, choices(game)).split("\n").at(-1));
  assert.equal(offer.state.fen, START_FEN);
  assert.equal(offer.state.position_occurrences, 1);
  assert.equal(offer.choices.length, 44);
  assert.match(prompt(game, choices(game)), /H=horse.*third appearance/);
  assert.deepEqual(decision('```json\n{"move":0}\n```', choices(game)), choices(game)[0]);
  for (const response of ['{"move":-1}', '{"move":44}', '{"move":"0"}', "A1"])
    assert.throws(() => decision(response, choices(game)));
  assert.throws(() => fromFen("9/9 w"));
});

test("chariot rays stop at friendly pieces and capture the first enemy", () => {
  const board = empty();
  board[5][4] = "R";
  board[5][6] = "P";
  board[3][4] = "p";
  const moves = pseudoMoves(board, 5, 4);
  assert(has(moves, 5, 5));
  assert(!has(moves, 5, 6));
  assert(has(moves, 3, 4));
  assert(!has(moves, 2, 4));
});

test("horse leg and elephant eye block their leaps, and elephants stay home", () => {
  const board = empty();
  board[5][4] = "H";
  board[4][4] = "P";
  assert(!has(pseudoMoves(board, 5, 4), 3, 3));
  assert(!has(pseudoMoves(board, 5, 4), 3, 5));
  assert(has(pseudoMoves(board, 5, 4), 4, 6));
  board[7][2] = "E";
  board[6][3] = "P";
  assert(!has(pseudoMoves(board, 7, 2), 5, 4));
  board[6][3] = ".";
  board[5][4] = ".";
  assert(has(pseudoMoves(board, 7, 2), 5, 4));
  board[5][4] = "E";
  assert(!has(pseudoMoves(board, 5, 4), 3, 6));
});

test("advisors and generals remain in the palace; open generals face", () => {
  const board = empty();
  board[9][3] = "A";
  board[9][4] = "K";
  board[0][4] = "k";
  assert.deepEqual(destinations(pseudoMoves(board, 9, 3)), ["8,4"]);
  assert(has(pseudoMoves(board, 9, 4), 0, 4));
  assert(inCheck(board, "red"));
  assert(inCheck(board, "black"));
  board[5][4] = "P";
  assert(!inCheck(board, "red"));
  assert(!inCheck(board, "black"));
  assert(!has(pseudoMoves(board, 9, 4), 7, 4));
});

test("cannon needs exactly one screen to capture and cannot land beyond it", () => {
  const board = empty();
  board[5][4] = "C";
  board[3][4] = "P";
  board[1][4] = "p";
  const moves = pseudoMoves(board, 5, 4);
  assert(has(moves, 4, 4));
  assert(!has(moves, 3, 4));
  assert(!has(moves, 2, 4));
  assert(has(moves, 1, 4));
  assert(!has(moves, 0, 4));
  board[3][4] = ".";
  assert(!has(pseudoMoves(board, 5, 4), 1, 4));
});

test("pawns move forward and gain sideways moves only after crossing the river", () => {
  const board = empty();
  board[6][4] = "P";
  board[3][4] = "p";
  assert.deepEqual(destinations(pseudoMoves(board, 6, 4)), ["5,4"]);
  assert.deepEqual(destinations(pseudoMoves(board, 3, 4)), ["4,4"]);
  board[6][4] = ".";
  board[4][4] = "P";
  assert.deepEqual(destinations(pseudoMoves(board, 4, 4)), ["3,4", "4,3", "4,5"]);
  board[3][4] = ".";
  board[5][4] = "p";
  assert.deepEqual(destinations(pseudoMoves(board, 5, 4)), ["5,3", "5,5", "6,4"]);
});

test("self-check and facing generals are filtered from legal moves", () => {
  const board = base();
  board[5][4] = "R";
  assert(!inCheck(board, "red"));
  assert(has(pseudoMoves(board, 5, 4), 5, 3));
  assert(!has(legalMoves(board, "red", 5, 4), 5, 3));
  assert(has(legalMoves(board, "red", 5, 4), 4, 4));
  const game = positioned(board);
  assert(!play(game, "red", 5, 4, 5, 3));
  assert.equal(game.moves.length, 0);
});

test("checkmate and stalemate both award the win to the moving side", () => {
  const mate = base();
  mate[2][4] = "P";
  mate[1][0] = "R";
  mate[2][3] = "R";
  mate[2][5] = "R";
  const checked = positioned(mate);
  assert(play(checked, "red", 1, 0, 1, 4));
  assert.equal(checked.phase, "over");
  assert.equal(checked.winner, "red");
  assert.equal(checked.reason, "checkmate");
  assert.equal(legalMoves(checked.board, "black").length, 0);

  const stale = base();
  stale[1][3] = "R";
  stale[1][8] = "R";
  const trapped = positioned(stale);
  assert(!inCheck(trapped.board, "black"));
  assert(play(trapped, "red", 1, 8, 1, 5));
  assert.equal(trapped.phase, "over");
  assert.equal(trapped.winner, "red");
  assert.equal(trapped.reason, "stalemate");
  assert(!trapped.check);
  assert.equal(legalMoves(trapped.board, "black").length, 0);
});

test("practice threefold draw counts board and side to move", () => {
  const board = base();
  board[9][0] = "R";
  board[0][8] = "r";
  const game = positioned(board);
  const cycle = [
    ["red", 9, 0, 8, 0], ["black", 0, 8, 1, 8],
    ["red", 8, 0, 9, 0], ["black", 1, 8, 0, 8],
  ];
  for (let repeat = 0; repeat < 2; repeat++)
    for (const [side, ...squares] of cycle) assert(play(game, side, ...squares));
  assert.equal(game.phase, "over");
  assert.equal(game.winner, null);
  assert.equal(game.reason, "repetition");
  assert.equal(game.moves.length, 8);
  assert(!play(game, "red", 9, 0, 8, 0));
});

test("captures replace the target and retain the captured piece in history", () => {
  const board = base();
  board[4][0] = "P";
  board[3][0] = "p";
  const game = positioned(board);
  assert(play(game, "red", 4, 0, 3, 0));
  assert.equal(game.board[3][0], "P");
  assert.equal(game.board[4][0], ".");
  assert.equal(game.moves[0].captured, "p");
  assert.equal(game.turn, "black");
});
