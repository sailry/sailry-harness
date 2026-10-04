import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { label, pattern, red } from "./cards.js";
import { portrait } from "./layout.js";

export function recentPlays(game) {
  return game.history.map((entry, index) => ({ ...entry, index }))
    .filter(entry => entry.cards.length > 0).slice(-40).reverse();
}

export function history(view) {
  const { game, text } = view, colors = theme().colors;
  return div().id("ddz-history").v_flex().flex_1().min_h(0).gap_3().p_4()
    .child(div().text_sm().font_semibold().child(text.playHistory))
    .child(div().id("ddz-history-list").v_flex().flex_1().min_h(0).gap_3().overflow_y_scroll()
      .children(recentPlays(game).map(entry =>
        div().id(`ddz-history-play-${entry.index}`).h_flex().flex_shrink(0).items_center().gap_3()
          .child(div().v_flex().flex_1().min_w(0).gap_1()
            .child(div().h_flex().flex_wrap().gap_1()
              .children(entry.cards.map(value => div().text_sm().font_medium()
                .text_color(red(value) ? colors.destructive : colors.foreground)
                .child(value >= 52 ? value === 52 ? text.smallJoker : text.bigJoker : label(value)))))
            .child(div().id(`ddz-history-pattern-${entry.index}`).text_sm().text_color(colors.muted_foreground)
              .child(text.patterns[pattern(entry.cards).kind])))
          .child(div().w(28).h(28).flex_shrink(0).tooltip(view.names[entry.player])
            .accessibility_label(view.names[entry.player])
            .child(Image.new(`ddz-history-avatar-${entry.index}`, { path: portrait(entry.player) }))))));
}
