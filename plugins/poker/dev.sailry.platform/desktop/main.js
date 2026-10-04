import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, prepare, forget, header_action } from "sailry";
import { messages } from "./locales.js";
import { act, choices, deal, legal, turn } from "./game.js";
import { errorCode, readSettings, listModels, resolveModel, modelCommand as command, modelChoice as select, completeRequest as completion } from "sailry/sdk";
import { Motion } from "./motion.js";
import { render } from "./view.js";
import { modal_closed, toast } from "sailry/ui";

export default class Poker extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.names = [this.text.you, this.text.names[Math.floor(Math.random() * this.text.names.length)]];
    this.raiseTo = 20;
    this.raiseId = 0;
    this.game = null;
    this.motion = new Motion();
    this.player = null;
    this.pending = null;
    this.busy = false;
    this.available = false;
    this.loading = false;
    this.error = null;
    this.elapsed = 0;
    this.tokens = 0;
    this.raiseOpen = false;
    this.resultDismissed = false;
    this.resultId = 0;
    cx.spawn(async cx => {
      try {
        while (true) {
          const id = await modal_closed();
          if (id === `poker-raise-${this.raiseId}`) { this.raiseOpen = false; cx.notify(); }
          if (id === `poker-result-${this.resultId}`) { this.resultDismissed = true; cx.notify(); }
        }
      } catch (_) { /* The modal channel closes with the mounted plugin. */ }
    });
    cx.spawn(async cx => {
      try {
        while (await header_action()) this.reset(cx);
      } catch (_) { /* The header closes with its mounted view. */ }
    });
    this.configure(cx);
    cx.spawn(async cx => {
      let cursor = "", entry = null, connected = true;
      try { while (true) {
        const change = await nextChange(cursor); cursor = change.cursor;
        const entered = entry !== null && entry !== change.entry;
        const resumed = !connected && change.connected;
        entry = change.entry; connected = change.connected;
        if (connected && (entered || resumed) && !this.game && !this.busy && !this.pending && !this.savePending) this.configure(cx);
      } } catch (_) { /* View release ends observation. */ }
    });
  }

  configure(cx) {
    if (this.loading) return;
    this.loading = true;
    this.available = false;
    this.error = null;
    cx.spawn(async cx => {
      try {
        const settings = await readSettings();
        this.names[0] = settings.values.player_name?.trim() || this.text.you;
        if (settings.values.player_1_enabled === false) { this.error = "configure"; return; }
        const catalog = await listModels();
        const name = settings.values.player_1?.trim();
        if (name) this.names[1] = name;
        this.player = resolveModel(settings, catalog, "player_1_model");
        this.available = true;
      } catch (error) {
        if (errorCode(error.message) === "configure") this.error = "configure";
        else this.fail("loadFailed");
      }
      finally { this.loading = false; cx.notify(); }
    });
  }

  fail(key) {
    this.error = key;
    toast({ id: "poker-error", kind: "error", message: this.text[key] });
  }

  setAmount() {
    const options = this.game && legal(this.game, 0);
    if (!options?.canRaise) return;
    this.raiseTo = Math.min(options.minTo, options.maxTo);
  }

  start(cx) {
    if (!this.available || this.loading || this.busy || this.pending) return;
    this.game = deal();
    this.beginHand(cx);
  }

  next(cx) {
    if (!this.game || this.game.phase !== "over" || this.game.settlement.matchOver
      || this.busy || this.pending) return;
    this.game = deal(Math.random, this.game.stacks, 1 - this.game.dealer, this.game.handNumber + 1);
    this.beginHand(cx);
  }

  reset(cx) {
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.pending = null;
    this.busy = false;
    if (!this.available || this.loading) { cx.notify(); return; }
    this.game = deal();
    this.beginHand(cx);
  }

  beginHand(cx) {
    this.raiseOpen = false;
    this.resultDismissed = false;
    this.resultId++;
    this.motion?.begin(this.game, cx);
    this.roundPlayer = { ...this.player };
    this.error = null;
    this.setAmount();
    this.advance(cx);
  }

  act(move, cx) {
    if (this.busy || !this.game || this.game.turn !== 0) return;
    if (!act(this.game, 0, move)) { this.fail("invalid"); cx.notify(); return; }
    this.raiseOpen = false;
    this.motion?.update(this.game, cx);
    this.error = null;
    this.advance(cx);
  }

  raise(cx) {
    if (this.busy || !this.game || this.game.phase !== "betting" || this.game.turn !== 0) return;
    if (!this.raiseOpen) return;
    this.act({ kind: "raise", to: this.raiseTo }, cx);
  }

  advance(cx) {
    const game = this.game;
    if (!game || game.phase !== "betting" || game.turn === 0 || this.busy) {
      this.setAmount();
      cx.notify();
      return;
    }
    this.busy = true;
    this.error = null;
    this.thinkingSince = Date.now();
    this.elapsed = 0;
    cx.notify();
    const current = () => this.game === game;
    const timer = this.thinkingTimer = cx.timer.every(1000, cx => {
      if (!current()) return;
      this.elapsed = Math.floor((Date.now() - this.thinkingSince) / 1000);
      cx.notify();
    });
    cx.spawn(async cx => {
      let request = this.pending;
      try {
        const moves = choices(game);
        if (!moves.length) throw new Error("invalidResponse");
        if (moves.length === 1 && !this.pending) {
          if (!act(game, 1, moves[0])) throw new Error("invalidResponse");
          this.motion?.update(game, cx);
          return;
        }
        if (!this.pending) this.pending = prepare(JSON.stringify(command(this.roundPlayer, turn(game, moves))));
        request = this.pending;
        const result = await completion(request);
        forget(request);
        request = null;
        if (!current()) return;
        this.pending = null;
        let move;
        const selected = select(result, this.roundPlayer, moves);
        move = selected.move;
        this.tokens += selected.tokens;
        if (!act(game, 1, move)) throw new Error("invalidResponse");
        this.motion?.update(game, cx);
      } catch (error) {
        if (current()) this.fail(this.pending ? "unconfirmed" : errorCode(error.message));
      } finally {
        timer.cancel();
        if (current()) {
          this.busy = false;
          this.setAmount();
          cx.notify();
          if (!this.error && game.phase === "betting" && game.turn === 1) this.advance(cx);
        }
        else if (request) forget(request);
      }
    });
  }

  render(cx) { return render(this, cx); }
}
