// Stone placement and the winning line use finite native transitions.
export class Motion {
  constructor() { this.reset(null); }

  reset(game) {
    this.game = game;
    this.moves = game?.moves.length || 0;
    this.stones = new Map();
    this.result = null;
  }

  update(game, cx) {
    if (this.game !== game) this.reset(game);
    if (!game) return;
    for (const move of game.moves.slice(this.moves)) {
      const key = `${move.row}-${move.column}`, entry = { ready: false };
      this.stones.set(key, entry);
      const current = () => this.game === game && this.stones.get(key) === entry;
      cx.spawn(async cx => {
        await cx.sleep(32);
        if (!current()) return;
        entry.ready = true;
        cx.notify();
        await cx.sleep(220);
        if (!current()) return;
        this.stones.delete(key);
        cx.notify();
      });
    }
    this.moves = game.moves.length;
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
