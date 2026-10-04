export const SIZE = 8;
const directions = [-1, 0, 1].flatMap(rise => [-1, 0, 1]
  .filter(across => rise !== 0 || across !== 0).map(across => [rise, across]));

function inside(row, column) {
  return Number.isInteger(row) && Number.isInteger(column)
    && row >= 0 && row < SIZE && column >= 0 && column < SIZE;
}

export function newGame(human = 1) {
  if (human !== 1 && human !== 2) throw new Error("Invalid player");
  const board = Array.from({ length: SIZE }, () => Array(SIZE).fill(0));
  board[3][3] = board[4][4] = 2;
  board[3][4] = board[4][3] = 1;
  return { board, human, turn: 1, phase: "play", winner: null, history: [], scored: false };
}

export function flips(board, player, row, column) {
  if ((player !== 1 && player !== 2) || !inside(row, column) || board[row][column] !== 0) return [];
  const captured = [];
  for (const [rise, across] of directions) {
    const line = [];
    let nextRow = row + rise, nextColumn = column + across;
    while (inside(nextRow, nextColumn) && board[nextRow][nextColumn] === 3 - player) {
      line.push({ row: nextRow, column: nextColumn });
      nextRow += rise;
      nextColumn += across;
    }
    if (line.length && inside(nextRow, nextColumn) && board[nextRow][nextColumn] === player)
      captured.push(...line);
  }
  return captured;
}

export function legalMoves(board, player) {
  const moves = [];
  for (let row = 0; row < SIZE; row++) for (let column = 0; column < SIZE; column++) {
    if (flips(board, player, row, column).length) moves.push({ row, column });
  }
  return moves;
}

export function counts(board) {
  const result = { black: 0, white: 0, empty: 0 };
  for (const row of board) for (const stone of row) {
    if (stone === 1) result.black++;
    else if (stone === 2) result.white++;
    else result.empty++;
  }
  return result;
}

export function play(game, player, row, column) {
  if (game.phase !== "play" || game.turn !== player) return false;
  const captured = flips(game.board, player, row, column);
  if (!captured.length) return false;
  game.board[row][column] = player;
  for (const position of captured) game.board[position.row][position.column] = player;
  game.history.push({ player, row, column, flipped: captured.length });
  const other = 3 - player;
  if (legalMoves(game.board, other).length) game.turn = other;
  else {
    game.history.push({ player: other, pass: true });
    if (legalMoves(game.board, player).length) game.turn = player;
    else {
      const score = counts(game.board);
      game.phase = "over";
      game.turn = null;
      game.winner = score.black === score.white ? 0 : score.black > score.white ? 1 : 2;
    }
  }
  return true;
}

export function choices(game) {
  return game.phase === "play" ? legalMoves(game.board, game.turn) : [];
}

export function coordinate(row, column) {
  return `${String.fromCharCode(65 + column)}${row + 1}`;
}

export function observation(game) {
  const last = game.history.findLast(entry => !entry.pass);
  return { board: game.board.map(row => row.map(stone => ".XO"[stone]).join("")),
    turn: game.turn === 1 ? "X" : "O", counts: counts(game.board),
    last: last ? coordinate(last.row, last.column) : null };
}

export function turn(game, moves) {
  return { instructions: "Play Reversi on an 8x8 board. X is black and moves first; O is white. Place a disc to enclose one or "
      + "more opposing discs in any of eight directions, then flip them. Players with no legal move pass. The "
      + "game ends when neither player can move; most discs wins. Rows are 1–8 and columns A–H. Pick one "
      + "strong offered legal move.",
    state: observation(game), choices: moves.map((move, id) => ({ id, at: coordinate(move.row, move.column) })) };
}
