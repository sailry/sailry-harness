export class Motion {
  constructor() {
    this.generation = 0;
    this.game = null;
    this.historyLength = 0;
    this.rolling = false;
    this.handShown = true;
    this.revealSeats = 0;
    this.resultShown = true;
    this.resultStarted = false;
    this.bidPulse = false;
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
    this.historyLength = 0;
    this.rolling = true;
    this.handShown = false;
    this.revealSeats = 0;
    this.resultShown = false;
    this.resultStarted = false;
    this.bidPulse = false;
    this.animate("rollTask", [[30, () => { this.rolling = false; }],
      [240, () => { this.handShown = true; }]], cx);
  }

  update(game, cx) {
    if (this.game !== game) return;
    if (game.history.length > this.historyLength) {
      this.historyLength = game.history.length;
      if (game.history.at(-1).kind === "bid") {
        this.bidPulse = true;
        this.animate("bidTask", [[220, () => { this.bidPulse = false; }]], cx);
      }
    }
    if (game.result && !this.resultStarted) {
      this.resultStarted = true;
      this.revealSeats = 0;
      this.resultShown = false;
      this.animate("resultTask", [[140, () => { this.revealSeats = 1; }],
        [140, () => { this.revealSeats = 2; }],
        [180, () => { this.resultShown = true; }]], cx);
    }
  }
}
