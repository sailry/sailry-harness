import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, prepare, forget, header_action } from "sailry";
import { messages } from "./locales.js";
import { newGame, play, choices, legalMoves, turn, sideOf } from "./game.js";
import { errorCode, readSettings, listModels, resolveModel, modelCommand as command, modelChoice as select, completeRequest as completion } from "sailry/sdk";
import { Motion } from "./motion.js";
import { render } from "./view.js";
import { toast } from "sailry/ui";

export default class Xiangqi extends View {
  init(_props, cx) {
    this.text = { ...messages(JSON.parse(context()).locale) };
    this.name = this.text.opponent;
    this.model = null;
    this.available = false;
    this.loading = false;
    this.game = null;
    this.motion = new Motion();
    this.selected = null;
    this.busy = false;
    this.pending = null;
    this.error = null;
    this.elapsed = 0;
    this.wins = 0;
    this.losses = 0;
    this.draws = 0;
    cx.spawn(async cx => {
      try {
        while (await header_action()) this.start(cx, this.game?.human || "red");
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
    toast({ id: "xiangqi-error", kind: "error", message: this.text[key] });
  }

  start(cx, human = "red") {
    if (!this.available || this.loading) return;
    this.thinkingTimer?.cancel();
    this.motion?.reset();
    if (!this.busy && this.pending) forget(this.pending);
    this.game = newGame(human);
    this.selected = null;
    this.pending = null;
    this.busy = false;
    this.error = null;
    this.elapsed = 0;
    this.advance(cx);
  }

  act(row, column, cx) {
    const game = this.game;
    if (!game || game.phase !== "play" || this.busy || game.turn !== game.human) return;
    if (sideOf(game.board[row]?.[column] || ".") === game.human) {
      this.selected = this.selected?.row === row && this.selected?.column === column
        ? null : { row, column };
      cx.notify();
      return;
    }
    if (!this.selected) return;
    const { row: fromRow, column: fromColumn } = this.selected;
    if (!legalMoves(game.board, game.human, fromRow, fromColumn)
      .some(move => move.toRow === row && move.toColumn === column)) return;
    if (!play(game, game.human, fromRow, fromColumn, row, column)) return;
    this.motion?.move(game.moves.at(-1), game.check, game.phase === "over", cx);
    this.selected = null;
    this.error = null;
    this.advance(cx);
  }

  settle() {
    const game = this.game;
    if (game?.phase !== "over" || game.scored) return;
    game.scored = true;
    if (game.winner === null) this.draws++;
    else if (game.winner === game.human) this.wins++;
    else this.losses++;
  }

  advance(cx) {
    this.settle();
    cx.notify();
    const game = this.game;
    if (!game || game.phase !== "play" || game.turn === game.human || this.busy) return;
    this.busy = true;
    this.error = null;
    this.elapsed = 0;
    this.thinkingSince = Date.now();
    const current = () => this.game === game;
    const timer = this.thinkingTimer = cx.timer.every(1000, cx => {
      if (!current()) return;
      this.elapsed = Math.floor((Date.now() - this.thinkingSince) / 1000);
      cx.notify();
    });
    cx.spawn(async cx => {
      let request = this.pending;
      try {
        if (!request && this.motion?.active?.move.side === game.human) {
          await cx.sleep(390);
          if (!current()) return;
        }
        const moves = choices(game);
        if (!request) this.pending = prepare(JSON.stringify(command(this.model, turn(game, moves))));
        request = this.pending;
        const result = await completion(request);
        if (!current()) return;
        forget(request);
        request = null;
        this.pending = null;
        const move = select(result, this.model, moves).move;
        if (!play(game, game.turn, move.fromRow, move.fromColumn, move.toRow, move.toColumn))
          throw new Error("invalidResponse");
        this.motion?.move(game.moves.at(-1), game.check, game.phase === "over", cx);
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
