import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, prepare, forget, header_action } from "sailry";
import { messages } from "./locales.js";
import { bid, challenge, choices, deal, legalBid, nextRound, turn } from "./game.js";
import { errorCode, readSettings, listModels, resolveModel, modelCommand as command, modelChoice as select, completeRequest as completion } from "sailry/sdk";
import { Motion } from "./motion.js";
import { render } from "./view.js";
import { toast } from "sailry/ui";

export default class LiarsDice extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    const names = [...this.text.names];
    this.names = [this.text.you, ...Array.from({ length: 2 }, () => names.splice(Math.floor(Math.random() * names.length), 1)[0])];
    this.game = null;
    this.motion = new Motion();
    this.players = null;
    this.quantity = 1;
    this.face = 1;
    this.pending = null;
    this.busy = false;
    this.available = false;
    this.loading = false;
    this.error = null;
    this.elapsed = 0;
    this.tokens = 0;
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
        if (!(this.enabled.slice(1).some(Boolean))) { this.error = "configure"; return; }
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
    toast({ id: "liars-error", kind: "error", message: this.text[key] });
  }

  selectBid() {
    if (!this.game || this.game.phase !== "bid" || this.game.turn !== 0) return;
    const current = this.game.currentBid;
    const total = this.game.counts.reduce((sum, count) => sum + count, 0);
    if (!current) { this.quantity = 1; this.face = 1; }
    else if (current.face < 6) { this.quantity = current.quantity; this.face = current.face + 1; }
    else if (current.quantity < total) { this.quantity = current.quantity + 1; this.face = 1; }
    else { this.quantity = total; this.face = 6; }
  }

  start(cx) {
    if (this.busy || this.pending || !this.available || this.loading) return;
    this.game = deal(Math.random, this.enabled.map(enabled => enabled ? 5 : 0));
    this.matchEnabled = [...this.enabled];
    this.motion?.begin(this.game, cx);
    this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
    this.error = null;
    this.selectBid();
    this.advance(cx);
  }

  next(cx) {
    if (!this.game || this.game.phase !== "round_over" || this.busy || this.pending) return;
    this.game = nextRound(this.game);
    this.motion?.begin(this.game, cx);
    this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
    this.error = null;
    this.selectBid();
    this.advance(cx);
  }

  reset(cx) {
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.pending = null;
    this.busy = false;
    if (!this.available || this.loading) { cx.notify(); return; }
    this.game = deal(Math.random, this.enabled.map(enabled => enabled ? 5 : 0));
    this.matchEnabled = [...this.enabled];
    this.motion?.begin(this.game, cx);
    this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
    this.error = null;
    this.selectBid();
    this.advance(cx);
  }

  setQuantity(value, cx) {
    if (!this.game || this.game.turn !== 0 || this.busy) return;
    const total = this.game.counts.reduce((sum, count) => sum + count, 0);
    this.quantity = Math.max(1, Math.min(total, value));
    this.error = null;
    cx.notify();
  }

  setFace(value, cx) {
    if (!this.game || this.game.turn !== 0 || this.busy) return;
    this.face = value;
    this.error = null;
    cx.notify();
  }

  act(kind, cx) {
    if (this.busy || !this.game || this.game.phase !== "bid" || this.game.turn !== 0) return;
    const accepted = kind === "challenge" ? challenge(this.game, 0)
      : bid(this.game, 0, this.quantity, this.face);
    if (!accepted) { this.fail("invalid"); cx.notify(); return; }
    this.motion?.update(this.game, cx);
    this.error = null;
    this.advance(cx);
  }

  advance(cx) {
    cx.notify();
    const game = this.game;
    if (!game || game.phase !== "bid" || game.turn === 0 || this.busy) {
      this.selectBid();
      return;
    }
    this.busy = true;
    this.error = null;
    const current = () => this.game === game;
    const timer = this.thinkingTimer = cx.timer.every(1000, cx => {
      if (!current()) return;
      this.elapsed = Math.floor((Date.now() - this.thinkingSince) / 1000);
      cx.notify();
    });
    cx.spawn(async cx => {
      let request = this.pending;
      try {
        while (current() && game.phase === "bid" && game.turn !== 0) {
          this.thinkingSince = Date.now();
          this.elapsed = 0;
          cx.notify();
          const moves = choices(game);
          if (!moves.length) throw new Error("invalidResponse");
          let move;
          if (moves.length === 1 && !this.pending) move = moves[0];
          else {
            const player = this.roundPlayers[game.turn - 1];
            if (!this.pending) this.pending = prepare(JSON.stringify(command(player, turn(game, moves))));
            request = this.pending;
            const result = await completion(request);
            forget(request);
            request = null;
            if (!current()) return;
            this.pending = null;
            const selected = select(result, player, moves);
            move = selected.move;
            this.tokens += selected.tokens;
          }
          const accepted = move.kind === "challenge" ? challenge(game, game.turn)
            : bid(game, game.turn, move.quantity, move.face);
          if (!accepted) throw new Error("invalidResponse");
          this.motion?.update(game, cx);
          cx.notify();
        }
      } catch (error) {
        if (current()) this.fail(this.pending ? "unconfirmed" : errorCode(error.message));
      } finally {
        timer.cancel();
        if (current()) { this.busy = false; this.selectBid(); cx.notify(); }
        else if (request) forget(request);
      }
    });
  }

  render(cx) { return render(this, cx); }
}
