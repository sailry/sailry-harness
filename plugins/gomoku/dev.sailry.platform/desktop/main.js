import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, prepare, forget, header_action } from "sailry";
import { messages } from "./locales.js";
import { newGame, play, choices, turn } from "./game.js";
import { errorCode, readSettings, listModels, resolveModel, modelCommand as command, modelChoice as select, completeRequest as completion } from "sailry/sdk";
import { render } from "./view.js";
import { Motion } from "./motion.js";
import { modal_closed, toast } from "sailry/ui";

export default class Gomoku extends View {
  init(_props, cx) {
    this.text = { ...messages(JSON.parse(context()).locale) };
    this.name = this.text.opponent;
    this.model = null;
    this.available = false;
    this.loading = false;
    this.game = null;
    this.motion = new Motion();
    this.busy = false;
    this.pending = null;
    this.error = null;
    this.elapsed = 0;
    this.wins = 0;
    this.losses = 0;
    this.draws = 0;
    this.resultDismissed = false;
    this.resultId = 0;
    cx.spawn(async cx => {
      try {
        while (true) {
          const id = await modal_closed();
          if (id === `gomoku-result-${this.resultId}`) { this.resultDismissed = true; cx.notify(); }
        }
      } catch (_) { /* The modal channel closes with the mounted plugin. */ }
    });
    cx.spawn(async cx => {
      try {
        while (await header_action()) this.start(cx, this.game?.human || 1);
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
        this.text.you = settings.values.player_name?.trim() || messages(JSON.parse(context()).locale).you;
        if (settings.values.ai_enabled === false) { this.error = "configure"; return; }
        const catalog = await listModels();
        this.model = resolveModel(settings, catalog, "ai_model");
        this.name = settings.values.ai_name?.trim() || this.text.opponent;
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
    toast({ id: "gomoku-error", kind: "error", message: this.text[key] });
  }

  start(cx, human = 1) {
    if (!this.available || this.loading) return;
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.game = newGame(human);
    this.resultDismissed = false;
    this.resultId = (this.resultId || 0) + 1;
    this.motion.reset(this.game);
    this.pending = null;
    this.busy = false;
    this.error = null;
    this.elapsed = 0;
    this.advance(cx);
  }

  act(row, column, cx) {
    if (!this.game || this.busy || this.game.turn !== this.game.human) return;
    if (!play(this.game, this.game.human, row, column)) return;
    this.error = null;
    this.advance(cx);
  }

  settle() {
    const game = this.game;
    if (game?.phase !== "over" || game.scored) return;
    game.scored = true;
    if (game.winner === 0) this.draws++;
    else if (game.winner === game.human) this.wins++;
    else this.losses++;
  }

  advance(cx) {
    this.motion.update(this.game, cx);
    this.settle();
    cx.notify();
    const game = this.game;
    if (!game || game.phase !== "play" || game.turn === game.human || this.busy) return;
    this.busy = true;
    this.error = null;
    this.elapsed = 0;
    this.thinkingSince = Date.now();
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
        if (!request) this.pending = prepare(JSON.stringify(command(this.model, turn(game, moves))));
        request = this.pending;
        const result = await completion(request);
        if (!current()) return;
        forget(request);
        request = null;
        this.pending = null;
        const move = select(result, this.model, moves).move;
        if (!play(game, game.turn, move.row, move.column)) throw new Error("invalidResponse");
        this.motion.update(game, cx);
        this.error = null;
        this.settle();
      } catch (error) {
        if (current()) this.fail(this.pending ? "unconfirmed" : errorCode(error.message));
      } finally {
        timer.cancel();
        if (current()) { this.busy = false; cx.notify(); }
        else if (request) forget(request);
      }
    });
  }

  render(cx) { return render(this, cx); }
}
