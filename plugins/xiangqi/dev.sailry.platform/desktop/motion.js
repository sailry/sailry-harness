// Script element transitions interpolate the piece between two board intersections.
export class Motion {
  constructor() { this.generation = 0; this.sequence = 0; this.active = null; this.resultVisible = false; }

  reset() {
    this.generation++;
    this.sequence++;
    this.active = null;
    this.resultVisible = false;
  }

  move(move, check, over, cx) {
    const generation = this.generation, sequence = ++this.sequence;
    this.active = { id: sequence, move, check, over, stage: 0 };
    this.resultVisible = false;
    cx.spawn(async cx => {
      for (const [delay, stage] of [[24, 1], [290, 2], [70, 3], [330, 4]]) {
        await cx.sleep(delay);
        if (this.generation !== generation || this.sequence !== sequence) return;
        if (stage === 4) this.active = null;
        else this.active.stage = stage;
        if (stage === 3 && over) this.resultVisible = true;
        cx.notify();
      }
    });
  }
}
