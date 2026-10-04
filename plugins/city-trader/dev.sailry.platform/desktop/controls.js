import { div } from "gpui-kit";
import { Button, ShimmerText, Icon } from "gpui-component";
import { Image, theme } from "sailry";
import { character } from "./art.js";
const retryable = ["failed", "timedOut", "invalidResponse", "unconfirmed"];

export function button(id, label, onClick, disabled = false, primary = false, icon = null, stretch = false) {
  const control = new Button(id).size("small").h(34).rounded_lg().min_w(84)
    .disabled(disabled).on_click((_event, cx) => onClick(cx));
  if (stretch) control.w_full();
  if (icon) control.w(32).h(32).min_w(32).p_0().rounded(16).tooltip(label)
    .child(new Icon(icon).size("small"));
  else control.label(label);
  return primary ? control.primary() : control.outline();
}

export function status(view) {
  const { game, text } = view, colors = theme().colors;
  const key = game.phase === "over" ? "result"
    : view.motion.rolling ? "rolling" : view.animating ? "moving"
    : view.thinking ? "thinking" : game.turn === 0 ? "yourTurn" : "paused";
  const line = div().id(`city-status-${key}`).h_flex().items_center().gap_2().min_w(0)
    .text_sm().font_medium().text_color(colors.foreground);
  if (game.turn !== 0 && game.phase !== "over") line
    .child(div().w(24).h(24).flex_shrink(0)
      .child(Image.new("city-status-avatar", { path: character(view.characters[game.turn], true) })));
  if (key === "thinking") line.child(new ShimmerText(text.thinking).id("city-thinking").text_sm())
    .child(div().flex_shrink(0).text_color(colors.muted_foreground).child(`${view.elapsed} ${text.seconds}`));
  else line.child(text[key] || text.phase[game.phase]);
  if (retryable.includes(view.error)) line.child(new Button("city-retry").ghost().size("small").h(24).px_1()
    .on_click((_event, cx) => view.advance(cx))
    .child(div().h_flex().items_center().gap_1().text_sm()
      .child(new Icon("icons/rotate-cw.svg").size("small")).child(text.retry)));
  return line;
}

export function action(view, move, index, first, primary = false, stretch = false) {
  const { text } = view, game = view.game;
  const disabled = !!(view.saving || view.savePending || view.busy || view.animating || view.pending || game.turn !== 0);
  let label = text.action[move.kind];
  if (move.kind === "bid") label = `${label} ${text.money(move.amount)}`;
  else if (move.kind === "propose")
    label = `${label} ${text.money(move.price)}`;
  else if (move.kind === "buy") label = `${label} ${text.money(game.board[game.players[0].position].price)}`;
  else if (move.kind === "upgrade") label = `${label} ${text.money(game.board[move.tile].upgrade)}`;
  else if (move.kind === "sell") {
    const tile = game.board[move.tile], level = game.properties[move.tile].level;
    label = `${label} ${text.money(Math.floor((tile.price + level * tile.upgrade) / 2))}`;
  }
  const id = !first && (move.kind === "bid" || move.kind === "propose")
    ? `city-action-${move.kind}-${index}` : `city-action-${move.kind}`;
  const control = button(`${id}-button`, label, cx => view.act(move, cx), disabled, primary, null, stretch);
  const item = div().id(id).child(control);
  return stretch ? item.flex_1().min_w(0) : item;
}
