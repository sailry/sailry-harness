export class Motion {
  constructor() {
    this.generation = 0;
    this.game = null;
    this.boardVisible = 0;
    this.historyLength = 0;
    this.holeVisible = true;
    this.resultShown = true;
    this.resultStarted = false;
    this.cue = null;
    this.cueShown = false;
    this.potPulse = false;
  }

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

  begin(game, cx) {
    this.generation++;
    this.game = game;
    this.boardVisible = 0;
    this.historyLength = 0;
    this.holeVisible = false;
    this.resultShown = false;
    this.resultStarted = false;
    this.cue = null;
    this.cueShown = false;
    this.potPulse = false;
    this.animate("dealTask", [[40, () => { this.holeVisible = true; }]], cx);
    this.update(game, cx);
  }

  update(game, cx) {
    if (this.game !== game) return;
    if (game.shown > this.boardVisible) {
      const count = game.shown - this.boardVisible;
      this.animate("boardTask", Array.from({ length: count }, () => [110, () => { this.boardVisible++; }]), cx);
    }
    if (game.history.length > this.historyLength) {
      const action = game.history.at(-1);
      this.historyLength = game.history.length;
      this.cue = action;
      this.cueShown = false;
      this.animate("cueTask", [[30, () => { this.cueShown = true; }],
        [560, () => { this.cueShown = false; }], [180, () => { this.cue = null; }]], cx);
      if (action.amount > 0) {
        this.potPulse = true;
        this.animate("potTask", [[220, () => { this.potPulse = false; }]], cx);
      }
    }
    if (game.phase === "over" && !this.resultStarted) {
      this.resultStarted = true;
      const remaining = Math.max(0, game.shown - this.boardVisible);
      this.animate("resultTask", [[Math.max(200, remaining * 110 + 140), () => {
        this.resultShown = true;
      }]], cx);
    }
  }
}
