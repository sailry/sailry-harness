export const SIZE = 15;
const directions = [[0, 1], [1, 0], [1, 1], [1, -1]];

export function newGame(human = 1) {
  if (human !== 1 && human !== 2) throw new Error("Invalid player");
  return { board: Array.from({ length: SIZE }, () => Array(SIZE).fill(0)), moves: [],
    turn: 1, human, winner: null, winning: [], phase: "play", scored: false };
}

function inside(row, column) {
  return Number.isInteger(row) && Number.isInteger(column)
    && row >= 0 && row < SIZE && column >= 0 && column < SIZE;
}

function run(board, row, column, player, rise, across) {
  const stones = [];
  let nextRow = row + rise, nextColumn = column + across;
  while (inside(nextRow, nextColumn) && board[nextRow][nextColumn] === player) {
    stones.push({ row: nextRow, column: nextColumn });
    nextRow += rise;
    nextColumn += across;
  }
  return stones;
}

export function winningLine(board, row, column, player) {
  for (const [rise, across] of directions) {
    const before = run(board, row, column, player, -rise, -across).reverse();
    const after = run(board, row, column, player, rise, across);
    const line = [...before, { row, column }, ...after];
    if (line.length >= 5) return line;
  }
  return [];
}

export function play(game, player, row, column) {
  if (game.phase !== "play" || game.turn !== player || !inside(row, column)
    || game.board[row][column] !== 0) return false;
  game.board[row][column] = player;
  game.moves.push({ player, row, column });
  game.winning = winningLine(game.board, row, column, player);
  if (game.winning.length) {
    game.phase = "over";
    game.winner = player;
  } else if (game.moves.length === SIZE * SIZE) {
    game.phase = "over";
    game.winner = 0;
  } else game.turn = 3 - player;
  return true;
}

export function coordinate(row, column) {
  return `${String.fromCharCode(65 + column)}${row + 1}`;
}

// Nearby intersections keep the model's offered actions compact as the board grows.
// Human moves remain legal on every empty intersection.
export function choices(game) {
  if (game.phase !== "play") return [];
  if (!game.moves.length) return [{ row: 7, column: 7 }];
  const candidates = new Set();
  for (const { row, column } of game.moves) {
    for (let rise = -2; rise <= 2; rise++) for (let across = -2; across <= 2; across++) {
      const nextRow = row + rise, nextColumn = column + across;
      if (inside(nextRow, nextColumn) && game.board[nextRow][nextColumn] === 0)
        candidates.add(nextRow * SIZE + nextColumn);
    }
  }
  return [...candidates].sort((left, right) => left - right)
    .map(index => ({ row: Math.floor(index / SIZE), column: index % SIZE }));
}

export function observation(game) {
  return { board: game.board.map(row => row.map(stone => ".XO"[stone]).join("")),
    turn: game.turn === 1 ? "X" : "O",
    last: game.moves.length ? coordinate(game.moves.at(-1).row, game.moves.at(-1).column) : null };
}

export function turn(game, moves) {
  return { instructions: "Play Gomoku on a 15×15 board. X is black and moves first; O is white. Five or more consecutive stones "
      + "in a row, column, or diagonal wins. Board rows are numbered 1–15 and columns A–O. Pick a strong move "
      + "from the offered empty intersections.",
    state: observation(game), choices: moves.map((move, id) => ({ id, at: coordinate(move.row, move.column) })) };
}
