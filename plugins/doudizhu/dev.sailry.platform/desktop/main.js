import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, prepare, forget, header_action } from "sailry";
import { messages } from "./locales.js";
import { deal, bid, play, choices, turn } from "./game.js";
import { Motion } from "./motion.js";
import { render } from "./view.js";
import { errorCode, readSettings, listModels, resolveModel, modelCommand as command, modelChoice as select, completeRequest as completion } from "sailry/sdk";
import { toast } from "sailry/ui";

export default class DouDizhu extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    const names = [...this.text.names];
    this.names = [this.text.you, ...Array.from({ length: 2 }, () => names.splice(Math.floor(Math.random() * names.length), 1)[0])];
    this.game = null;
    this.motion = new Motion();
    this.selected = [];
    this.drag = null;
    this.skipClick = false;
    this.hintPass = false;
    this.busy = false;
    this.elapsed = 0;
    this.error = null;
    this.pending = null;
    this.tokens = 0;
    this.score = 0;
    this.round = 0;
    this.available = false;
    this.loading = false;
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
        this.enabled = [true, settings.values.player_1_enabled !== false, settings.values.player_2_enabled !== false];
        if (!(this.enabled.slice(1).every(Boolean))) { this.error = "configure"; return; }
        const catalog = await listModels();
        this.players = [1, 2].map(player => {
          const key = `player_${player}`;
          if (!this.enabled[player]) return null;
          const playerModel = resolveModel(settings, catalog, `${key}_model`);
          const name = settings.values[key]?.trim();
          if (name) this.names[player] = name;
          return playerModel;
        });
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
    toast({ id: "ddz-error", kind: "error", message: this.text[key] });
  }

  start(cx) {
    if (this.busy || this.pending || !this.available || this.loading) return;
    this.motion?.begin(cx);
    this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
    this.game = deal(Math.random, this.round++ % 3);
    this.selected = [];
    this.drag = null;
    this.skipClick = false;
    this.hintPass = false;
    this.error = null;
    this.advance(cx);
  }

  reset(cx) {
    this.score = 0;
    if (!this.available || this.loading) { cx.notify(); return; }
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.pending = null;
    this.busy = false;
    this.round = 0;
    this.start(cx);
  }

  act(value, cx) {
    if (this.busy || this.game.turn !== 0) return;
    const accepted = this.game.phase === "bid" ? bid(this.game, 0, value) : play(this.game, 0, value);
    if (!accepted) { this.fail("invalid"); cx.notify(); return; }
    this.selected = [];
    this.drag = null;
    this.skipClick = false;
    this.hintPass = false;
    this.error = null;
    this.advance(cx);
  }

  settle() {
    if (this.game.phase !== "over" || this.game.scored) return;
    this.game.scored = true;
    this.selected = [];
    this.drag = null;
    const won = (this.game.winner === this.game.landlord) === (this.game.landlord === 0);
    this.game.won = won;
    this.game.points = (won ? 1 : -1) * this.game.bid * this.game.multiplier * (this.game.landlord === 0 ? 2 : 1);
    this.score += this.game.points;
  }

  advance(cx) {
    this.settle();
    this.motion?.update(this.game, cx);
    cx.notify();
    if (this.busy || !["bid", "play"].includes(this.game.phase) || this.game.turn === 0) return;
    this.busy = true;
    this.error = null;
    const game = this.game;
    const current = () => this.game === game;
    const timer = this.thinkingTimer = cx.timer.every(1000, cx => {
      if (!current()) return;
      this.elapsed = Math.floor((Date.now() - this.thinkingSince) / 1000);
      cx.notify();
    });
    cx.spawn(async cx => {
      let request = this.pending;
      try {
        while (current() && ["bid", "play"].includes(this.game.phase) && this.game.turn !== 0) {
          this.thinkingSince = Date.now();
          this.elapsed = 0;
          cx.notify();
          const moves = choices(this.game);
          // Forced actions need no model decision; recover any admitted request first.
          if (moves.length === 1 && !this.pending) {
            const accepted = this.game.phase === "bid" ? bid(this.game, this.game.turn, moves[0])
              : play(this.game, this.game.turn, moves[0]);
            if (!accepted) throw new Error("invalidResponse");
            this.settle();
            this.motion?.update(this.game, cx);
            cx.notify();
            continue;
          }
          const player = this.roundPlayers[this.game.turn - 1];
          if (!this.pending) this.pending = prepare(JSON.stringify(command(player, turn(this.game, moves))));
          request = this.pending;
          const result = await completion(request);
          forget(request);
          request = null;
          if (!current()) return;
          this.pending = null;
          let move;
          const selected = select(result, player, moves);
          move = selected.move;
          this.tokens += selected.tokens;
          const accepted = this.game.phase === "bid" ? bid(this.game, this.game.turn, move)
            : play(this.game, this.game.turn, move);
          if (!accepted) throw new Error("invalidResponse");
          this.settle();
          this.motion?.update(this.game, cx);
          cx.notify();
        }
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
