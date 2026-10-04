import { View } from "gpui-kit";
import { nextChange } from "sailry/sdk";
import { context, header_action } from "sailry";
import { errorCode, readSettings, listModels, resolveModel, prepareModel, completeRequest, forgetRequest as forget, modelChoice as select, getValue, setValue } from "sailry/sdk";
import { messages } from "./locales.js";
import { newGame, restore, choices, act as applyAction, turn } from "./game.js";
import { Motion, presentEvents } from "./motion.js";
import { render } from "./view.js";
import { decisionTile } from "./details.js";
import { validCharacters } from "./art.js";
import { modal_closed, toast } from "sailry/ui";

export default class CityTrader extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.names = [this.text.you, ...this.text.names.slice(0, 2)];
    this.game = null;
    this.motion = new Motion();
    this.players = null;
    this.characters = [0, 1, 2];
    this.editingCharacter = 0;
    this.dialogOpen = false;
    this.dialogKind = "property";
    this.dialogId = 0;
    cx.spawn(async cx => {
      try {
        while (true) {
          const id = await modal_closed();
          if (id === `city-dialog-${this.dialogId}`) this.closeDetails(cx);
        }
      } catch (_) { /* The modal channel closes with the mounted plugin. */ }
    });
    this.selected = null;
    this.expanded = [0];
    this.decision = null;
    this.pending = null;
    this.busy = false;
    this.animating = false;
    this.thinking = false;
    this.available = false;
    this.loading = false;
    this.error = null;
    this.elapsed = 0;
    this.tokens = 0;
    this.saved = null;
    this.savedAtHistory = null;
    this.saving = false;
    this.savePending = null;
    this.saveError = null;
    cx.spawn(async cx => {
      try {
        while (true) {
          const action = await header_action();
          if (!action) break;
          if (action === "city-header-save") this.saveGame(cx);
          else this.showLobby(cx);
        }
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
        this.players = [1, 2].map(seat => {
          const key = `player_${seat}`;
          if (!this.enabled[seat]) return null;
          const player = resolveModel(settings, catalog, `${key}_model`);
          const name = settings.values[key]?.trim();
          if (name) this.names[seat] = name;
          return player;
        });
        this.saved = await getValue("match");
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
    toast({ id: "city-error", kind: "error", message: this.text[key] });
  }

  failSave(key) {
    this.saveError = key;
    toast({ id: "city-save", kind: "error", message: this.text[key] });
  }

  start(cx) {
    if (this.saving || this.busy || this.animating || this.pending || !this.available || this.loading) return;
    this.reset(cx);
  }

  reset(cx) {
    if (this.saving || this.savePending) return;
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.pending = null;
    this.busy = false;
    this.animating = false;
    this.thinking = false;
    if (!this.available || this.loading) { cx.notify(); return; }
    this.closeDetails();
    this.game = newGame();
    this.game.players.forEach((player, seat) => { player.enabled = this.enabled[seat]; });
    this.decision = null;
    this.expanded = [0];
    this.savedAtHistory = null;
    this.saveError = null;
    this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
    this.motion.reset(this.game);
    this.selected = null;
    this.error = null;
    this.elapsed = 0;
    this.tokens = 0;
    this.advance(cx);
  }

  resume(cx) {
    if (!this.saved?.value || this.busy || this.saving) return;
    const value = this.saved.value;
    try {
      const game = restore(value.game);
      if (game.players.some((player, seat) => seat > 0 && player.enabled && !this.players[seat - 1])) {
        this.failSave("configure"); cx.notify(); return;
      }
      if (!Array.isArray(value.names) || value.names.length !== 3 || value.names.some(name => typeof name !== "string")) throw new Error("invalidSave");
      if (!validCharacters(value.characters)) throw new Error("invalidSave");
      this.closeDetails();
      this.characters = [...value.characters];
      this.decision = null;
      this.expanded = [0];
      this.game = game;
      this.names = [...value.names];
      this.savedAtHistory = game.history.length;
      this.saveError = null;
      this.roundPlayers = this.players.map(player => player ? ({ ...player }) : null);
      this.motion.reset(this.game);
      this.selected = null;
      this.error = null;
      this.advance(cx);
      cx.notify();
    } catch (_) { this.failSave("invalidSave"); cx.notify(); }
  }

  saveGame(cx) {
    if (!this.game || (this.game.phase !== "over" && this.game.turn !== 0)
      || this.busy || this.animating || this.pending || this.saving) return;
    this.saving = true;
    this.saveError = null;
    cx.notify();
    cx.spawn(async cx => {
      try {
        if (!this.savePending) this.savePending = setValue("match", {
          game: this.game, names: this.names, characters: this.characters,
        }, this.saved?.revision || "0");
        const result = await completeRequest(this.savePending);
        forget(this.savePending);
        this.savePending = null;
        if (result.Err) throw new Error(result.Err.code === "revision_conflict" ? "saveConflict" : "saveFailed");
        this.saved = result.Ok.data;
        this.savedAtHistory = this.game.history.length;
        toast({ id: "city-save", kind: "info", message: this.text.saved });
      } catch (error) {
        this.failSave(this.savePending ? "saveUnconfirmed" : error.message === "saveConflict" ? "saveConflict" : "saveFailed");
      } finally { this.saving = false; cx.notify(); }
    });
  }

  refreshSave(cx) {
    if (this.saving || this.savePending) return;
    this.saving = true;
    cx.spawn(async cx => {
      try {
        this.saved = await getValue("match");
        this.saveError = null;
      } catch (_) { this.failSave("saveFailed"); }
      finally { this.saving = false; cx.notify(); }
    });
  }

  chooseCharacter(index, cx) {
    if (this.game || !Number.isInteger(index) || index < 0 || index >= 6) return;
    const seat = this.editingCharacter, previous = this.characters[seat];
    const other = this.characters.indexOf(index);
    if (other !== -1) this.characters[other] = previous;
    this.characters[seat] = index;
    cx.notify();
  }

  showLobby(cx) {
    if (this.saving || this.savePending) return;
    this.thinkingTimer?.cancel();
    if (!this.busy && this.pending) forget(this.pending);
    this.closeDetails();
    this.game = null;
    this.motion.reset(null);
    this.pending = null;
    this.busy = false;
    this.animating = false;
    this.thinking = false;
    this.error = null;
    this.decision = null;
    cx.notify();
  }

  closeDetails(cx) {
    this.dialogOpen = false;
    const feedback = this.feedback;
    this.feedback = null;
    feedback?.finish();
    cx?.notify();
  }

  openDetails(kind = "property") {
    this.closeDetails();
    this.dialogKind = kind;
    this.dialogId++;
    this.dialogOpen = true;
  }

  showResult(cx) {
    if (this.game?.phase !== "over") return;
    this.openDetails("result");
    cx.notify();
  }

  syncDecision() {
    const game = this.game;
    if (!game || this.busy || this.animating) return;
    if (game.phase !== "over" && (game.turn !== 0
      || !["buy", "auction", "trade", "debt"].includes(game.phase))) return;
    const key = `${game.phase}-${game.history.length}`;
    if (this.decision === key) return;
    this.decision = key;
    this.selected = decisionTile(this);
    this.openDetails(game.phase === "over" ? "result" : "property");
  }

  inspect(tile, cx) {
    if (!this.game || !Number.isInteger(tile) || tile < 0 || tile >= this.game.properties.length) return;
    this.selected = tile;
    this.closeDetails();
    this.openDetails();
    cx.notify();
  }

  act(action, cx) {
    const game = this.game;
    if (!game || game.phase === "over" || game.turn !== 0 || this.saving || this.savePending || this.busy || this.animating || this.pending) return;
    const history = game.history.length;
    if (!applyAction(game, action)) return;
    const entries = game.history.slice(history);
    this.closeDetails();
    this.error = null;
    this.motion.update(game, cx);
    this.animating = this.motion.moving;
    cx.notify();
    cx.spawn(async cx => {
      await this.motion.settle(cx);
      if (this.game !== game) return;
      await presentEvents(this, game, entries, cx);
      if (this.game !== game) return;
      this.animating = false;
      this.advance(cx);
    });
  }

  advance(cx) {
    const game = this.game;
    if (game?.phase === "manage" && game.turn === 0 && !this.busy && !this.animating
      && !this.saving && !this.savePending && !this.pending) {
      applyAction(game, { kind: "end" });
      this.motion.update(game, cx);
    }
    this.syncDecision();
    cx.notify();
    if (!game || game.phase === "over" || game.turn === 0 || this.busy || this.animating) return;
    this.busy = true;
    this.error = null;
    const current = () => this.game === game;
    const timer = this.thinkingTimer = cx.timer.every(1000, cx => {
      if (!current() || !this.thinking) return;
      this.elapsed = Math.floor((Date.now() - this.thinkingSince) / 1000);
      cx.notify();
    });
    cx.spawn(async cx => {
      let request = this.pending;
      try {
        while (current() && game.phase !== "over" && game.turn !== 0) {
          const moves = choices(game);
          if (!moves.length) throw new Error("invalidResponse");
          let action;
          if (!this.pending && game.phase === "roll") action = moves.find(move => move.kind === "roll");
          else if (moves.length === 1 && !this.pending) action = moves[0];
          else {
            this.thinking = true;
            this.thinkingSince = Date.now();
            this.elapsed = 0;
            cx.notify();
            const player = this.roundPlayers[game.turn - 1];
            if (!this.pending) this.pending = prepareModel(player, turn(game, moves));
            request = this.pending;
            const result = await completeRequest(request);
            forget(request);
            request = null;
            if (!current()) return;
            this.pending = null;
            const selected = select(result, player, moves);
            action = selected.move;
            this.tokens += selected.tokens;
            this.thinking = false;
          }
          const history = game.history.length;
          if (!applyAction(game, action)) throw new Error("invalidResponse");
          const entries = game.history.slice(history);
          this.motion.update(game, cx);
          this.animating = this.motion.moving;
          cx.notify();
          await this.motion.settle(cx);
          if (!current()) return;
          await presentEvents(this, game, entries, cx);
          if (!current()) return;
          this.animating = false;
        }
      } catch (error) {
        if (current()) this.fail(this.pending ? "unconfirmed" : errorCode(error.message));
      } finally {
        timer.cancel();
        if (current()) {
          this.busy = false;
          this.animating = false;
          this.thinking = false;
          if (!this.error) this.advance(cx);
          else this.syncDecision();
          cx.notify();
        } else if (request) forget(request);
      }
    });
  }

  render(cx) { return render(this, cx); }
}
