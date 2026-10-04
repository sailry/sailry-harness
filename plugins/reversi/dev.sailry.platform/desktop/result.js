import { div } from "gpui-kit";
import { Image } from "sailry";
import { portrait } from "./layout.js";
import { action, piece } from "./controls.js";
import { counts } from "./game.js";

export function result(view) {
  const { game, text } = view;
  const outcome = game.winner === 0 ? "draw" : game.winner === game.human ? "win" : "lose";
  const score = counts(game.board);
  return div().id("reversi-result").w(320).v_flex().items_center().gap_5()
    .child(div().h_flex().justify_center().gap_3()
      .children((game.winner === 0 ? [game.human, 3 - game.human] : [game.winner]).map(player =>
        div().w(72).h(72).accessibility_label(player === game.human ? text.you : view.name)
          .child(Image.new(`reversi-winner-${player}`, { path: portrait(view, player) })))))
    .child(div().id(`reversi-result-${outcome}`).text_2xl().font_bold().child(text[outcome]))
    .child(div().h_flex().items_center().justify_center().gap_4().text_2xl().font_semibold()
      .child(piece(1, 24)).child(String(score.black)).child(piece(2, 24)).child(String(score.white)))
    .child(div().h_flex().justify_center().gap_2()
      .child(action("reversi-view-board", text.viewBoard, cx => { view.resultDismissed = true; cx.notify(); }).outline())
      .child(action("reversi-play-again", text.again, cx => view.start(cx, game.human)).primary()));
}
