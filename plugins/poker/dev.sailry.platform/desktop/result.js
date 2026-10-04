import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { portrait } from "./layout.js";
import { button } from "./controls.js";

export function result(view) {
  const { game, text } = view, colors = theme().colors;
  const { winner, reason, values, pot, net, matchOver } = game.settlement;
  const outcome = winner === null ? "split" : winner === 0 ? "won" : "lost";
  const types = [text.highCard, text.pair, text.twoPair, text.trips, text.straight,
    text.flush, text.fullHouse, text.quads, text.straightFlush];
  return div().id("poker-result-dialog").w(360).max_h(Math.max(280, window.viewport_size().height - 100))
    .v_flex().items_center().gap_4().overflow_y_scroll()
    .child(div().h_flex().justify_center().gap_3()
      .children((winner === null ? [0, 1] : [winner]).map(player => div().w(72).h(72)
        .accessibility_label(view.names[player])
        .child(Image.new(`poker-winner-${player}`, { path: portrait(player) })))))
    .child(div().id(`poker-result-${outcome}`).text_2xl().font_bold().child(text[outcome]))
    .child(div().id("poker-result-net").text_3xl().font_bold().text_color(net < 0 ? colors.destructive : colors.foreground)
      .child(`${net > 0 ? "+" : ""}${net}`))
    .child(div().w_full().v_flex().gap_2().py_3().border_y_1().border_color(colors.border)
      .child(div().h_flex().justify_between().text_sm()
        .child(div().text_color(colors.muted_foreground).child(text.pot)).child(String(pot)))
      .children(reason === "showdown" ? [0, 1].map(player =>
        div().id(`poker-result-hand-${player}`).h_flex().items_center().justify_between().gap_3()
          .child(div().w(24).h(24).accessibility_label(view.names[player])
            .child(Image.new(`poker-result-avatar-${player}`, { path: portrait(player) })))
          .child(div().text_sm().child(types[values[player][0]])))
        : []))
    .child(div().w_full().h_flex().justify_center().gap_2()
      .child(button("poker-close-result", text.viewTable, cx => { view.resultDismissed = true; cx.notify(); }))
      .child(button(matchOver ? "poker-new-match" : "poker-next-hand", matchOver ? text.newMatch : text.next,
        cx => matchOver ? view.reset(cx) : view.next(cx), "primary")));
}
