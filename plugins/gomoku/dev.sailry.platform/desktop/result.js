import { div } from "gpui-kit";
import { Image } from "sailry";
import { portrait } from "./layout.js";
import { action } from "./controls.js";

export function result(view) {
  const { game, text } = view;
  const outcome = game.winner === 0 ? "draw" : game.winner === game.human ? "win" : "lose";
  return div().id("gomoku-result").w(320).v_flex().items_center().gap_5()
    .child(div().h_flex().justify_center().gap_3()
      .children((game.winner === 0 ? [game.human, 3 - game.human] : [game.winner]).map(player =>
        div().w(72).h(72).accessibility_label(player === game.human ? text.you : view.name)
          .child(Image.new(`gomoku-winner-${player}`, { path: portrait(view, player) })))))
    .child(div().id(`gomoku-result-${outcome}`).text_2xl().font_bold().child(text[outcome]))
    .child(div().h_flex().justify_center().gap_2()
      .child(action("gomoku-view-board", text.viewBoard, cx => { view.resultDismissed = true; cx.notify(); }).outline())
      .child(action("gomoku-play-again", text.again, cx => view.start(cx, game.human)).primary()));
}
