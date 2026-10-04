// Native transitions interpolate newly played cards and round effects.
export class Motion {
  constructor() { this.generation = 0; this.turn = null; this.plays = 0; }

  animate(key, steps, cx) {
    const generation = this.generation;
    const token = {};
    this[key] = token;
    cx.spawn(async cx => {
      for (const [delay, update] of steps) {
        await cx.sleep(delay);
        if (this.generation !== generation || this[key] !== token) return;
        update();
        cx.notify();
      }
    });
  }

  begin(cx) {
    this.generation++;
    this.turn = null; this.plays = 0;
    this.fireworks = null; this.resultReady = false; this.play = null;
    this.dealing = 0;
    this.animate("dealTask", [[30, () => this.dealing = 1], [180, () => this.dealing = 2],
      [180, () => this.dealing = 3], [240, () => this.dealing = null]], cx);
  }

  update(game, cx) {
    if (game.history.length !== this.plays) {
      const latest = game.history.at(-1);
      if (latest.cards.length) {
        this.play = latest; this.entered = false;
        this.animate("playTask", [[30, () => this.entered = true]], cx);
      }
      this.plays = game.history.length;
    }
    const turn = `${game.phase}-${game.turn}`;
    if (turn === this.turn) return;
    this.turn = turn;
    if (game.phase === "over") {
      this.resultReady = false;
      this.animate("resultTask", [[2000, () => { this.resultReady = true; this.fireworks = game.won ? 0 : null; }],
        ...(game.won ? [[40, () => this.fireworks = 1], [620, () => this.fireworks = 2],
          [350, () => this.fireworks = null]] : [])], cx);
    }
  }
}
