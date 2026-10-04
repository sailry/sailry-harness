import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { portrait } from "./layout.js";

export function history(view) {
  const { game, text } = view, colors = theme().colors;
  return div().id("poker-history").v_flex().flex_1().min_h(0).gap_3().p_4()
    .child(div().text_sm().font_semibold().child(text.history))
    .child(div().id("poker-history-list").v_flex().flex_1().min_h(0).gap_3().overflow_y_scroll()
      .children(game.history.map((entry, index) => ({ ...entry, index })).slice(-40).reverse().map(entry =>
        div().id(`poker-history-action-${entry.index}`).h_flex().flex_shrink(0).items_center().gap_3()
          .child(div().v_flex().flex_1().min_w(0).gap_0().text_sm().line_height(1.25)
            .child(div().font_medium().child(entry.kind === "raise" ? `${text.raiseTo} ${entry.to}`
              : entry.amount ? `${text[entry.kind]} ${entry.amount}` : text[entry.kind]))
            .child(div().text_color(colors.muted_foreground).child(text[entry.stage])))
          .child(div().w(28).h(28).flex_shrink(0).accessibility_label(view.names[entry.player])
            .child(Image.new(`poker-history-avatar-${entry.index}`, { path: portrait(entry.player) }))))));
}
