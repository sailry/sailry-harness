import { div } from "gpui-kit";
import { Button, ShimmerText } from "gpui-component";
import { Image, theme } from "sailry";
import { material } from "./board.js";
import { portrait } from "./layout.js";
import { retryable } from "./model.js";

export function action(id, label, onClick) {
  return new Button(id).size("small").h(36).rounded_lg().min_w(92)
    .label(label).on_click((_event, cx) => onClick(cx));
}

export function piece(color, size = 18) {
  return div().w(size).h(size).flex_shrink(0).rounded(9999)
    .bg(color === 1 ? material.black : material.white)
    .border_1().border_color(color === 1 ? theme().colors.muted_foreground : material.edge).shadow_sm();
}

export function status(view) {
  const { game, text } = view, colors = theme().colors;
  const over = game.phase === "over";
  const key = over ? game.winner === 0 ? "draw" : game.winner === game.human ? "win" : "lose"
    : view.busy ? "thinking" : game.turn === game.human ? "yourTurn" : "paused";
  const display = div().id(`gomoku-status-${key}`).h_flex().flex_wrap().justify_center().items_center().gap_2().min_h(36)
    .text_sm().text_color(colors.muted_foreground);
  if (over) return display.child(action("gomoku-new-game", text.again, cx => view.start(cx, game.human)).primary());
  if (key === "thinking") display
    .child(div().w(24).h(24).flex_shrink(0).accessibility_label(game.turn === game.human ? text.you : view.name)
      .child(Image.new("gomoku-status-avatar", { path: portrait(view, game.turn) })))
    .child(new ShimmerText(text.thinking).id("gomoku-thinking").text_sm())
    .child(div().id("gomoku-elapsed").text_xs().child(`${view.elapsed} ${text.seconds}`));
  else display.child(text[key]);
  if (retryable.includes(view.error)) display.child(action("gomoku-retry", text.retry,
    cx => view.advance(cx)).outline().h(28));
  return display;
}
