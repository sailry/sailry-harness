// Present settled tile effects after movement, before the next decision or turn.
export async function presentEvents(view, game, entries, cx) {
  for (const entry of entries.filter(entry => ["event", "bonus", "tax", "passed_start", "rent"].includes(entry.key))) {
    if (view.game !== game) return;
    const due = entries.some(other => other.key === "debt" && other.seat === entry.seat
      && other.reason === entry.key && other.tile === entry.tile);
    const amount = ["tax", "rent"].includes(entry.key) ? -entry.amount : entry.amount;
    view.openDetails("event");
    view.animating = true;
    const feedback = view.feedback = { entry, amount, due, visible: false };
    const dismissed = new Promise(resolve => { feedback.finish = resolve; });
    cx.notify();
    cx.spawn(async cx => {
      await cx.sleep(24);
      if (view.feedback !== feedback) return;
      feedback.visible = true;
      cx.notify();
      await cx.sleep(2000);
      if (view.feedback === feedback) view.closeDetails(cx);
    });
    await dismissed;
    if (view.dialogOpen) return;
  }
}

// Native transitions keep each board step smooth without rerendering every frame.
export class Motion {
  constructor() {
    this.generation = 0;
    this.rollSequence = 0;
    this.game = null;
    this.positions = [0, 0, 0];
    this.cash = [0, 0, 0];
    this.cashDelta = [null, null, null];
    this.historyLength = 0;
    this.rollSeq = 0;
    this.dice = [1, 1];
    this.diceFrame = 0;
    this.rolling = false;
    this.settling = false;
    this.moving = false;
    this.highlight = null;
    this.resultVisible = false;
    this.resultStarted = false;
    this.pending = null;
    this.finish = null;
  }

  reset(game) {
    this.finish?.();
    this.generation++;
    this.rollSequence++;
    this.game = game;
    this.positions = game?.players.map(player => player.position) || [0, 0, 0];
    this.cash = game?.players.map(player => player.cash) || [0, 0, 0];
    this.cashDelta = [null, null, null];
    this.historyLength = game?.history.length || 0;
    this.rollSeq = game?.lastRoll?.seq || 0;
    this.dice = game?.lastRoll?.dice || [1, 1];
    this.diceFrame = 0;
    this.rolling = false;
    this.settling = false;
    this.moving = false;
    this.highlight = null;
    this.resultVisible = game?.phase === "over";
    this.resultStarted = this.resultVisible;
    this.pending = null;
    this.finish = null;
  }

  settle(_cx) { return this.pending || Promise.resolve(); }

  update(game, cx) {
    if (this.game !== game) return;
    const generation = this.generation;
    for (let seat = 0; seat < game.players.length; seat++) {
      const difference = game.players[seat].cash - this.cash[seat];
      this.cash[seat] = game.players[seat].cash;
      if (difference) this.flashCash(seat, difference, cx);
    }
    for (const entry of game.history.slice(this.historyLength)) {
      if (["buy", "rent", "auction_won", "upgrade", "sell", "trade_proposed",
        "trade_accepted"].includes(entry.key) && Number.isInteger(entry.tile)) {
        this.flashTile(entry.tile, cx);
      }
    }
    this.historyLength = game.history.length;
    const roll = game.lastRoll;
    if (roll && roll.seq !== this.rollSeq) {
      this.rollSeq = roll.seq;
      this.animateRoll(roll, game, cx);
    } else if (!this.moving) {
      this.positions = game.players.map(player => player.position);
    }
    if (game.phase === "over" && !this.resultStarted) {
      this.resultStarted = true;
      this.resultVisible = false;
      cx.spawn(async cx => {
        await this.settle(cx);
        await cx.sleep(30);
        if (this.generation !== generation || this.game !== game) return;
        this.resultVisible = true;
        cx.notify();
      });
    }
  }

  animateRoll(roll, game, cx) {
    this.finish?.();
    const generation = this.generation, sequence = ++this.rollSequence;
    this.pending = new Promise(resolve => { this.finish = resolve; });
    const finish = this.finish;
    this.positions[roll.seat] = roll.from;
    this.rolling = true;
    this.settling = false;
    this.diceFrame = 0;
    this.moving = true;
    const current = () => this.generation === generation && this.rollSequence === sequence
      && this.game === game;
    cx.spawn(async cx => {
      for (let frame = 0; frame < 20; frame++) {
        this.diceFrame = frame;
        cx.notify();
        await cx.sleep(40);
        if (!current()) return;
      }
      this.dice = [...roll.dice];
      this.settling = true;
      for (let frame = 0; frame < 8; frame++) {
        this.diceFrame = frame;
        cx.notify();
        await cx.sleep(40);
        if (!current()) return;
      }
      this.rolling = false;
      this.settling = false;
      cx.notify();
      const steps = roll.dice[0] + roll.dice[1];
      for (let step = 1; step <= steps; step++) {
        this.positions[roll.seat] = (roll.from + step) % game.properties.length;
        cx.notify();
        await cx.sleep(145);
        if (!current()) return;
      }
      this.positions[roll.seat] = roll.to;
      this.moving = false;
      this.pending = null;
      this.finish = null;
      finish();
      cx.notify();
    });
  }

  flashCash(seat, amount, cx) {
    const generation = this.generation;
    const marker = { amount, visible: false };
    this.cashDelta[seat] = marker;
    cx.spawn(async cx => {
      await cx.sleep(24);
      if (this.generation !== generation || this.cashDelta[seat] !== marker) return;
      marker.visible = true;
      cx.notify();
      await cx.sleep(700);
      if (this.generation !== generation || this.cashDelta[seat] !== marker) return;
      marker.visible = false;
      cx.notify();
      await cx.sleep(230);
      if (this.generation !== generation || this.cashDelta[seat] !== marker) return;
      this.cashDelta[seat] = null;
      cx.notify();
    });
  }

  flashTile(tile, cx) {
    const generation = this.generation;
    const marker = { tile, phase: 0 };
    this.highlight = marker;
    cx.spawn(async cx => {
      await cx.sleep(30);
      if (this.generation !== generation || this.highlight !== marker) return;
      marker.phase = 1;
      cx.notify();
      await cx.sleep(460);
      if (this.generation !== generation || this.highlight !== marker) return;
      this.highlight = null;
      cx.notify();
    });
  }
}
