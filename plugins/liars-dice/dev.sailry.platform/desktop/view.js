import { Header, theme } from "sailry";
import { div } from "gpui-kit";
import { Button, Spinner, Tag, ShimmerText } from "gpui-component";
import { legalBid } from "./game.js";
import { die, table } from "./table.js";
import { retryable } from "./model.js";

function button(id, label, action, primary = false) {
  const control = new Button(id).size("small").h(34).rounded(9999).min_w(64)
    .on_click((_event, cx) => action(cx)).label(label);
  return primary ? control.primary() : control.outline();
}

function lobby(view) {
  const { colors } = theme(), text = view.text;
  const status = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const hero = div().h_flex().items_center().justify_center().gap_2()
    .w(220).h(98).rounded(55).bg("#584336").p_3()
    .child(div().h_flex().items_center().justify_center().gap_2().size_full()
      .rounded(48).bg("#1a5943")
      .child(die(2)).child(die(5)).child(die(6)));
  const panel = div().id(`liars-status-${status}`).v_flex().items_center().justify_between().gap_4()
    .w_full().max_w(390).min_h(350).p_6()
    .child(hero)
    .child(div().v_flex().items_center().gap_1()
      .child(div().text_3xl().font_bold().text_color(colors.foreground).child(text.title))
      .child(div().text_sm().text_color(colors.muted_foreground).child(text.subtitle)))
    .child(div().text_center().text_sm().text_color(colors.muted_foreground).child(text.rule));
  if (view.loading) panel.child(new Spinner());
  else if (view.available) panel.child(button("liars-start", text.start, cx => view.start(cx), true).w_full().h(40));
  else if (status !== "loadFailed") panel.child(div().text_sm().text_color(colors.muted_foreground).child(text[status]));
  return div().id("liars-lobby").size_full().v_flex().items_center().justify_center().p_6().child(panel);
}

function controls(view) {
  const { game, text } = view, { colors } = theme();
  const controls = div().id("liars-controls").v_flex().items_center().gap_2().w_full().max_w(920).mx_auto();
  if (retryable.includes(view.error)) controls.child(button("liars-retry", text.retry, cx => view.advance(cx)));
  if (game.phase !== "bid") {
    const winner = game.result.winner;
    if (winner !== null) controls.child(div().id("liars-match-winner").text_lg().font_bold()
      .text_color(colors.foreground).child(text.winner(view.names[winner])));
    return controls.child(div().id("liars-action-next")
      .child(button(game.phase === "match_over" ? "liars-again" : "liars-next",
        game.phase === "match_over" ? text.again : text.next,
        cx => game.phase === "match_over" ? view.reset(cx) : view.next(cx), true)));
  }
  if (view.busy) return controls.child(div().id("liars-status-thinking").h_flex().items_center().gap_2()
    .child(new ShimmerText(`${view.names[game.turn]} ${text.thinking}`).text_sm())
    .child(div().text_xs().text_color(colors.muted_foreground).child(`${view.elapsed} ${text.seconds}`)));
  if (game.turn !== 0) return controls;
  const total = game.counts.reduce((sum, count) => sum + count, 0);
  const quantity = div().id("liars-quantity").h_flex().items_center().gap_2()
    .child(div().text_sm().text_color(colors.muted_foreground).child(text.quantity))
    .child(button("liars-quantity-down", text.decrease, cx => view.setQuantity(view.quantity - 1, cx))
      .disabled(view.quantity <= 1))
    .child(div().min_w(28).text_center().font_bold().child(String(view.quantity)))
    .child(button("liars-quantity-up", text.increase, cx => view.setQuantity(view.quantity + 1, cx))
      .disabled(view.quantity >= total));
  const faces = div().id("liars-faces").h_flex().items_center().justify_center().flex_wrap().gap_1()
    .child(div().text_sm().text_color(colors.muted_foreground).child(text.face))
    .children(Array.from({ length: 6 }, (_, index) => button(`liars-face-${index + 1}`,
      String(index + 1), cx => view.setFace(index + 1, cx), view.face === index + 1)));
  const actions = div().h_flex().items_center().justify_center().flex_wrap().gap_2()
    .child(div().id("liars-action-bid")
      .child(button("liars-bid", text.bid, cx => view.act("bid", cx), true)
        .disabled(!legalBid(game, view.quantity, view.face))));
  if (game.currentBid) actions.child(div().id("liars-action-challenge")
    .child(button("liars-challenge", text.challenge, cx => view.act("challenge", cx))));
  return controls.child(quantity).child(faces).child(actions);
}

export function render(view) {
  const { colors } = theme();
  const header = Header.new("liars-header", { content: JSON.stringify({ items: [
    { label: view.text.round, value: String(view.game?.round || 0) },
    { label: view.text.dice, value: String(view.game?.counts[0] ?? 5) },
  ], actions: [{ id: "liars-reset", label: view.text.resetScore, icon: "icons/rotate-cw.svg" }] }) });
  if (!view.game) return lobby(view).child(header);
  const game = view.game;
  const compact = window.viewport_size().width < 760 || window.viewport_size().height < 900;
  const status = game.phase === "match_over" ? view.text.matchOver
    : game.phase === "round_over" ? view.text.roundOver
    : view.busy ? `${view.names[game.turn]} ${view.text.thinking}`
    : game.turn === 0 ? view.text.yourTurn : view.text.waiting;
  const statusId = game.phase === "match_over" ? "matchOver"
    : game.phase === "round_over" ? "roundOver"
    : view.busy ? "thinking"
    : game.turn === 0 ? "yourTurn" : "waiting";
  const meta = div().h_flex().items_center().justify_between().gap_2().w_full().max_w(920).mx_auto()
    .child(div().text_sm().text_color(colors.muted_foreground).child(view.text.subtitle))
    .child(div().id(`liars-status-${statusId}`)
      .child(new Tag().variant(game.turn === 0 && game.phase === "bid" ? "info" : "secondary")
        .size("small").child(status)));
  return div().id("liars-table").size_full().min_w_0().min_h_0().overflow_y_scroll()
    .child(div().v_flex().items_center().w_full().min_h(compact ? 580 : 680).p_4().gap_3()
      .child(meta).child(table(view, compact)).child(controls(view)))
    .child(header);
}
