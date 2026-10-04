import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { coordinate } from "./game.js";
import { portrait } from "./layout.js";

export function history(view) {
  const { game, text } = view, colors = theme().colors;
  return div().id("gomoku-history").v_flex().w(200).h_full().flex_shrink(0)
    .border_l_1().border_color(colors.border).p_4().gap_3()
    .child(div().text_sm().font_semibold().child(text.moves))
    .child(div().v_flex().flex_1().min_h(0).overflow_y_scroll().gap_3()
      .children(game.moves.map((entry, index) => ({ ...entry, index })).slice(-40).reverse().map(entry =>
        div().id(`gomoku-move-${entry.index + 1}`).h_flex().flex_shrink(0).items_center().gap_3()
          .child(div().v_flex().flex_1().min_w(0).gap_0().text_sm().line_height(1.25)
            .child(div().font_medium().child(coordinate(entry.row, entry.column)))
            .child(div().text_color(colors.muted_foreground).child(entry.player === 1 ? text.black : text.white)))
          .child(div().w(28).h(28).flex_shrink(0)
            .accessibility_label(entry.player === game.human ? text.you : view.name)
            .child(Image.new(`gomoku-history-avatar-${entry.index + 1}`, { path: portrait(view, entry.player) }))))));
}
