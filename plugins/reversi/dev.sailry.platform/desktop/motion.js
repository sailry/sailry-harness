// Only presentation targets change here; native GPUI frames interpolate them.
export class Motion {
  constructor() { this.reset(null); }

  reset(game) {
    this.game = game;
    this.board = game?.board.map(row => [...row]);
    this.discs = new Map();
    this.result = null;
  }

  update(game, cx) {
    if (this.game !== game) this.reset(game);
    if (!game) return;
    for (let row = 0; row < game.board.length; row++) {
      for (let column = 0; column < game.board[row].length; column++) {
        const value = game.board[row][column], before = this.board[row][column];
        if (value === before) continue;
        const key = `${row}-${column}`, previous = this.discs.get(key);
        const entry = { from: previous && previous.phase < 2 ? previous.from : before,
          value, placed: before === 0, phase: 0 };
        this.discs.set(key, entry);
        const current = () => this.game === game && this.discs.get(key) === entry;
        cx.spawn(async cx => {
          await cx.sleep(32);
          if (!current()) return;
          entry.phase = 1;
          cx.notify();
          if (!entry.placed) {
            await cx.sleep(110);
            if (!current()) return;
            entry.phase = 2;
            cx.notify();
          }
          await cx.sleep(180);
          if (!current()) return;
          this.discs.delete(key);
          cx.notify();
        });
      }
    }
    this.board = game.board.map(row => [...row]);
    if (game.phase === "over" && !this.result) {
      const result = this.result = { visible: false };
      cx.spawn(async cx => {
        await cx.sleep(32);
        if (this.game !== game || this.result !== result) return;
        result.visible = true;
        cx.notify();
      });
    }
  }
}
