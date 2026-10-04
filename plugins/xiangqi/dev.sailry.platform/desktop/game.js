export const ROWS = 10;
export const COLUMNS = 9;
export const START_FEN = "rheakaehr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RHEAKAEHR w - - 0 1";
const directions = [[-1, 0], [1, 0], [0, -1], [0, 1]];

export const other = side => side === "red" ? "black" : "red";
export const sideOf = piece => piece === "." ? null
  : piece === piece.toUpperCase() ? "red" : "black";
const typeOf = piece => piece.toLowerCase();
const inside = (row, column) => Number.isInteger(row) && Number.isInteger(column)
  && row >= 0 && row < ROWS && column >= 0 && column < COLUMNS;
const palace = (row, column, side) => column >= 3 && column <= 5
  && (side === "red" ? row >= 7 && row <= 9 : row >= 0 && row <= 2);
const crossed = (row, side) => side === "red" ? row <= 4 : row >= 5;

export function fromFen(fen) {
  const [ranks, active] = fen.trim().split(/\s+/);
  if (active !== "w" && active !== "b") throw new Error("Invalid FEN turn");
  const rows = ranks?.split("/");
  if (rows?.length !== ROWS) throw new Error("Invalid FEN ranks");
  const board = rows.map(rank => {
    const cells = [];
    for (const token of rank) {
      if (/^[1-9]$/.test(token)) cells.push(...Array(Number(token)).fill("."));
      else if (/^[rheakcpRHEAKCP]$/.test(token)) cells.push(token);
      else throw new Error("Invalid FEN piece");
    }
    if (cells.length !== COLUMNS) throw new Error("Invalid FEN width");
    return cells;
  });
  if (board.flat().filter(piece => piece === "K").length !== 1
    || board.flat().filter(piece => piece === "k").length !== 1)
    throw new Error("Invalid FEN generals");
  return { board, turn: active === "w" ? "red" : "black" };
}

export function toFen(board, turn) {
  if (turn !== "red" && turn !== "black") throw new Error("Invalid side");
  const ranks = board.map(row => {
    if (row.length !== COLUMNS) throw new Error("Invalid board width");
    let text = "", empty = 0;
    for (const piece of row) {
      if (piece === ".") empty++;
      else {
        if (empty) text += String(empty);
        text += piece;
        empty = 0;
      }
    }
    return text + (empty ? String(empty) : "");
  });
  if (ranks.length !== ROWS) throw new Error("Invalid board height");
  return `${ranks.join("/")} ${turn === "red" ? "w" : "b"} - - 0 1`;
}

export function newGame(human = "red", fen = START_FEN) {
  if (human !== "red" && human !== "black") throw new Error("Invalid player");
  const { board, turn } = fromFen(fen);
  const key = toFen(board, turn);
  return { board, turn, human, moves: [], repetitions: { [key]: 1 },
    phase: "play", winner: null, reason: null, check: inCheck(board, turn), scored: false };
}

function add(board, side, fromRow, fromColumn, toRow, toColumn, moves) {
  if (!inside(toRow, toColumn) || sideOf(board[toRow][toColumn]) === side) return;
  moves.push({ fromRow, fromColumn, toRow, toColumn,
    piece: board[fromRow][fromColumn], captured: board[toRow][toColumn] });
}

function ray(board, side, row, column, moves, cannon) {
  for (const [rise, across] of directions) {
    let screened = false;
    for (let nextRow = row + rise, nextColumn = column + across;
      inside(nextRow, nextColumn); nextRow += rise, nextColumn += across) {
      const occupant = board[nextRow][nextColumn];
      if (cannon && screened) {
        if (occupant !== ".") {
          if (sideOf(occupant) !== side) add(board, side, row, column, nextRow, nextColumn, moves);
          break;
        }
      } else if (occupant === ".") {
        add(board, side, row, column, nextRow, nextColumn, moves);
      } else if (cannon) screened = true;
      else {
        if (sideOf(occupant) !== side) add(board, side, row, column, nextRow, nextColumn, moves);
        break;
      }
    }
  }
}

export function pseudoMoves(board, row, column) {
  if (!inside(row, column)) return [];
  const piece = board[row][column], side = sideOf(piece), moves = [];
  if (!side) return moves;
  const offer = (toRow, toColumn) => add(board, side, row, column, toRow, toColumn, moves);
  switch (typeOf(piece)) {
    case "r": ray(board, side, row, column, moves, false); break;
    case "c": ray(board, side, row, column, moves, true); break;
    case "h":
      for (const [rise, across] of directions) {
        if (board[row + rise]?.[column + across] !== ".") continue;
        if (rise) {
          offer(row + rise * 2, column - 1);
          offer(row + rise * 2, column + 1);
        } else {
          offer(row - 1, column + across * 2);
          offer(row + 1, column + across * 2);
        }
      }
      break;
    case "e":
      for (const rise of [-2, 2]) for (const across of [-2, 2]) {
        const nextRow = row + rise, nextColumn = column + across;
        if (inside(nextRow, nextColumn) && board[row + rise / 2][column + across / 2] === "."
          && (side === "red" ? nextRow >= 5 : nextRow <= 4)) offer(nextRow, nextColumn);
      }
      break;
    case "a":
      for (const rise of [-1, 1]) for (const across of [-1, 1])
        if (palace(row + rise, column + across, side)) offer(row + rise, column + across);
      break;
    case "k":
      for (const [rise, across] of directions)
        if (palace(row + rise, column + across, side)) offer(row + rise, column + across);
      for (const rise of [-1, 1]) {
        for (let nextRow = row + rise; nextRow >= 0 && nextRow < ROWS; nextRow += rise) {
          const occupant = board[nextRow][column];
          if (occupant === ".") continue;
          if (typeOf(occupant) === "k" && sideOf(occupant) !== side)
            offer(nextRow, column);
          break;
        }
      }
      break;
    case "p":
      offer(row + (side === "red" ? -1 : 1), column);
      if (crossed(row, side)) { offer(row, column - 1); offer(row, column + 1); }
      break;
  }
  return moves;
}

function moved(board, move) {
  const next = board.map(row => [...row]);
  next[move.toRow][move.toColumn] = next[move.fromRow][move.fromColumn];
  next[move.fromRow][move.fromColumn] = ".";
  return next;
}

export function inCheck(board, side) {
  const general = side === "red" ? "K" : "k";
  let position = null;
  for (let row = 0; row < ROWS; row++) for (let column = 0; column < COLUMNS; column++)
    if (board[row][column] === general) position = { row, column };
  if (!position) return true;
  for (let row = 0; row < ROWS; row++) for (let column = 0; column < COLUMNS; column++) {
    if (sideOf(board[row][column]) !== other(side)) continue;
    if (pseudoMoves(board, row, column).some(move => move.toRow === position.row
      && move.toColumn === position.column)) return true;
  }
  return false;
}

export function legalMoves(board, side, fromRow = null, fromColumn = null) {
  if (side !== "red" && side !== "black") return [];
  const rows = fromRow === null ? Array.from({ length: ROWS }, (_, index) => index) : [fromRow];
  const moves = [];
  for (const row of rows) {
    if (!inside(row, fromColumn === null ? 0 : fromColumn)) continue;
    const columns = fromColumn === null ? Array.from({ length: COLUMNS }, (_, index) => index) : [fromColumn];
    for (const column of columns) {
      if (sideOf(board[row][column]) !== side) continue;
      for (const move of pseudoMoves(board, row, column))
        if (!inCheck(moved(board, move), side)) moves.push(move);
    }
  }
  return moves;
}

export function play(game, side, fromRow, fromColumn, toRow, toColumn) {
  if (game.phase !== "play" || game.turn !== side || !inside(fromRow, fromColumn)
    || !inside(toRow, toColumn)) return false;
  const move = legalMoves(game.board, side, fromRow, fromColumn)
    .find(candidate => candidate.toRow === toRow && candidate.toColumn === toColumn);
  if (!move) return false;
  game.board = moved(game.board, move);
  game.turn = other(side);
  game.check = inCheck(game.board, game.turn);
  game.moves.push({ ...move, side, check: game.check });
  if (typeOf(move.captured) === "k") {
    game.phase = "over";
    game.winner = side;
    game.reason = "checkmate";
  } else if (!legalMoves(game.board, game.turn).length) {
    game.phase = "over";
    game.winner = side;
    game.reason = game.check ? "checkmate" : "stalemate";
  } else {
    const key = toFen(game.board, game.turn);
    game.repetitions[key] = (game.repetitions[key] || 0) + 1;
    if (game.repetitions[key] >= 3) {
      game.phase = "over";
      game.winner = null;
      game.reason = "repetition";
    }
  }
  return true;
}

export const coordinate = (row, column) => `${String.fromCharCode(65 + column)}${ROWS - row}`;
export const choices = game => game.phase === "play" ? legalMoves(game.board, game.turn) : [];

export function observation(game) {
  const last = game.moves.at(-1);
  const fen = toFen(game.board, game.turn);
  return { fen, turn: game.turn, position_occurrences: game.repetitions[fen] || 0,
    last: last ? `${coordinate(last.fromRow, last.fromColumn)}-${coordinate(last.toRow, last.toColumn)}` : null,
    in_check: game.check };
}

export function turn(game, moves) {
  return { instructions: "Play Xiangqi (Chinese chess). Red moves first. The board has 9 files A-I and 10 ranks 1-10; rank 1 is "
      + "red's back rank. FEN uses uppercase pieces for red, lowercase for black, and w for red to move. "
      + "Pieces: R=chariot, H=horse, E=elephant, A=advisor, K=general, C=cannon, P=pawn. In this casual "
      + "practice game, the third appearance of the same board and side to move is a draw. The offered choices "
      + "are all legal; select one strong move.",
    state: observation(game), choices: moves.map((move, id) => ({ id,
      from: coordinate(move.fromRow, move.fromColumn),
      to: coordinate(move.toRow, move.toColumn), piece: move.piece,
      capture: move.captured === "." ? null : move.captured })) };
}
