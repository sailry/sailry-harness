import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { character, building } from "./art.js";
import { button } from "./controls.js";

export function result(view) {
  const { game, text } = view, colors = theme().colors;
  const outcome = !game.winners.includes(0) ? "defeat" : game.winners.length > 1 ? "tied" : "victory";
  const order = game.result.scores.map((score, seat) => ({ score, seat }))
    .filter(({ seat }) => game.players[seat].enabled)
    .sort((a, b) => b.score - a.score);
  return div().id("city-result-dialog").w(400)
    .max_h(Math.max(300, window.viewport_size().height - 100)).overflow_y_scroll()
    .v_flex().gap_4()
    .child(div().relative().top(view.motion.resultVisible ? 0 : 8)
      .opacity(view.motion.resultVisible ? 1 : 0)
      .transition("top", { duration: 280, easing: "ease-out" })
      .transition("opacity", { duration: 280, easing: "ease-out" })
      .v_flex().items_center().gap_2()
      .child(div().relative().h(152).w_full().h_flex().justify_center().items_end()
        .children(game.winners.map(seat => div().w(116).h(140)
          .child(Image.new(`city-winner-art-${seat}`, { path: character(view.characters[seat]) }))))
        .child(div().absolute().top(0).right(48).w(48).h(48)
          .child(Image.new("city-result-star", { path: building({ kind: "bonus" }) }))))
      .child(div().id(`city-outcome-${outcome}`).text_2xl().font_bold().child(text[outcome]))
      .child(div().text_sm().text_color(colors.muted_foreground)
        .child(`${game.winners.map(seat => view.names[seat]).join(" · ")} · ${text.winner}`)))
    .child(div().h_flex().justify_between().text_xs().text_color(colors.muted_foreground)
      .child(text.result).child(text.netWorth))
    .child(div().v_flex().gap_2().children(order.map(({ score, seat }) =>
      div().id(`city-result-seat-${seat}`).h_flex().items_center().gap_3().p_3().rounded_lg()
        .bg(game.winners.includes(seat) ? colors.accent : colors.muted)
        .child(div().w(16).text_sm().text_color(colors.muted_foreground).child(String(order.findIndex(entry => entry.score === score) + 1)))
        .child(div().w(36).h(36).child(Image.new(`city-result-portrait-${seat}`, { path: character(view.characters[seat], true) })))
        .child(div().flex_1().text_sm().font_medium().child(view.names[seat]))
        .child(div().text_sm().font_semibold().child(text.money(score))))))
    .child(div().h_flex().justify_end().gap_2().pt_2()
      .child(button("city-close-result", text.viewBoard, cx => view.closeDetails(cx)))
      .child(button("city-again", text.playAgain, cx => view.showLobby(cx), view.saving || !!view.savePending, true)));
}
