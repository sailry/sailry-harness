import { Header, theme } from "sailry";
import { Modal } from "sailry/ui";
import { div } from "gpui-kit";
import { Spinner } from "gpui-component";
import { card, back, felt, seat, hole } from "./table.js";
import { stage } from "./layout.js";
import { button, actionRegion, raiseDialog } from "./controls.js";
import { history } from "./history.js";
import { result } from "./result.js";
import { legal } from "./game.js";

function lobby(view) {
  const { colors } = theme(), text = view.text;
  const status = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const hero = div().w(216).h(106).h_flex().items_center().justify_center().gap_2()
    .child(back(0)).child(card(47)).child(card(24));
  const panel = div().id(`poker-status-${status}`).v_flex().items_center().justify_between().gap_4()
    .w_full().max_w(380).min_h(330).p_6()
    .child(hero)
    .child(div().v_flex().items_center().gap_1()
      .child(div().text_3xl().font_bold().text_color(colors.foreground).child(text.title))
      .child(div().text_sm().text_color(colors.muted_foreground)
        .child(text.subtitle)));
  if (view.loading) panel.child(new Spinner());
  else if (view.available) panel.child(button("poker-start", text.start, cx => view.start(cx), "primary").w_full().h(40));
  else if (status !== "loadFailed") panel.child(div().text_sm().text_color(colors.muted_foreground).child(text[status]));
  return div().id("poker-lobby").size_full().v_flex().items_center().justify_center().p_6().child(panel);
}

export function render(view) {
  const { game, text } = view, colors = theme().colors;
  const header = Header.new("poker-header", { content: JSON.stringify({ items: [
    { label: text.score, value: String(game?.stacks[0] ?? 1000) },
    { label: text.hand, value: String(game?.handNumber ?? 0) },
  ], actions: [{ id: "poker-reset", label: text.resetScore, icon: "icons/rotate-cw.svg" }] }) });
  if (!game) return lobby(view).child(header);
  const compact = window.viewport_size().height < 760;
  const bodyHeight = Math.max(448, Math.min(788, window.viewport_size().height - 196));
  const pot = game.phase === "over" ? game.settlement.pot : game.pot;
  const info = div().id("poker-info").w(200).h_full().flex_shrink(0).v_flex().border_l_1().border_color(colors.border)
    .child(div().id("poker-pot").v_flex().gap_2().p_4().border_b_1().border_color(colors.border)
      .child(div().text_sm().font_semibold().child(text.pot))
      .child(div().id("poker-pot-value").relative().text_2xl().font_semibold()
        .transition("top", { duration: 220, easing: "ease-out" })
        .top(view.motion?.potPulse ? -4 : 0).child(String(pot)))
      .child(div().text_xs().text_color(colors.muted_foreground).child(text.blinds)))
    .child(history(view));
  const content = div().id("poker-content").v_flex().w_full().max_w(1240).min_w(720)
    .child(div().id("poker-player-strip").h_flex().h(89).flex_shrink(0).border_b_1().border_color(colors.border)
      .child(seat(view, 0)).child(seat(view, 1)))
    .child(div().h_flex().h(bodyHeight).flex_shrink(0).items_stretch()
      .child(div().v_flex().h_full().flex_1().min_w(0)
        .child(felt(view, compact, actionRegion(view)))
        .child(div().id("poker-hand-region").w_full().flex_shrink(0).p_4().border_t_1().border_color(colors.border)
          .child(hole(view, 0, compact))))
      .child(info));
  const showResult = game.phase === "over" && view.motion?.resultShown !== false && !view.resultDismissed;
  const showRaise = view.raiseOpen && !view.busy && legal(game, 0)?.canRaise;
  return div().id("poker-table").size_full().v_flex().min_w(0).min_h(0).overflow_hidden()
    .text_color(colors.foreground).child(header).child(stage(content, 720))
    .child(Modal.new(showRaise ? `poker-raise-${view.raiseId}` : `poker-result-${view.resultId}`,
      { open: !!(showRaise || showResult) })
      .children(showRaise ? [raiseDialog(view)] : showResult ? [result(view)] : []));
}
