import { Header, Image, theme } from "sailry";
import { div } from "gpui-kit";
import { Button as IconButton } from "gpui-base";
import { Button, Spinner, ShimmerText, Icon, Tag } from "gpui-component";
import { choices, tablePlay } from "./game.js";
import { legal, pattern, beats } from "./cards.js";
import { card, cards, seat, backs } from "./table.js";
import { effects, fireworks, tween } from "./effects.js";
import { stage, portrait } from "./layout.js";
import { history } from "./history.js";
import { hand } from "./hand.js";
import { retryable } from "./model.js";

const button = (id, label, action, highlight = false) => {
  const control = new Button(id).size("small").h(34).rounded_lg().min_w(64)
    .on_click((_event, cx) => action(cx));
  return highlight ? control.primary().label(label) : control.label(label);
};

function hero(view, cx, won = null) {
  const display = div().relative().w(168).h(104);
  const faces = won === false ? [0, 5, 10] : [44, 45, 53];
  for (let index = 0; index < faces.length; index++) {
    display.child(div().absolute().left(index * 48).top(index === 1 ? 0 : 12)
      .child(card(faces[index], cx, view.text)));
  }
  return display;
}

function lobby(view, cx) {
  const { text } = view, colors = theme().colors;
  const status = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const content = div().id(`ddz-status-${status}`).v_flex().items_center().justify_between()
    .w_full().max_w(360).h(320).p_6().text_center()
    .child(hero(view, cx))
    .child(div().v_flex().items_center().gap_1()
      .child(div().text_3xl().font_bold().text_color(colors.foreground).child(text.title))
      .child(div().text_sm().text_color(colors.muted_foreground).child(text.subtitle)));
  if (view.loading) content.child(new Spinner());
  else if (view.available) content.child(button("ddz-deal", text.start, cx => view.start(cx)).primary().w_full().h(40));
  else if (status === "loadFailed") content.child(button("ddz-retry", text.retry, cx => view.configure(cx)));
  else if (status !== "loadFailed") content.child(div().text_sm().text_color(colors.muted_foreground).child(text[status]));
  return div().id("ddz-lobby").size_full().v_flex().items_center().justify_center().p_6().child(content);
}

function result(view, cx) {
  const { game, text } = view, colors = theme().colors;
  return div().id("ddz-result-overlay").absolute().inset_0().v_flex().items_center().justify_center()
    .bg(colors.surface).p_4()
    .child(div().id(game.won ? "ddz-result-win" : "ddz-result-lose").v_flex().items_center().justify_between()
      .w_full().max_w(360).h(360).p_6().rounded_2xl().bg(colors.background)
      .border_1().border_color(colors.border).shadow_xl()
      .child(hero(view, cx, game.won))
      .child(new Tag().variant(game.won ? "success" : "danger").outline().bg("#00000000").border_0().p_0()
        .h(40).text_3xl().font_bold().child(game.won ? text.victory : text.defeat))
      .child(div().h_flex().items_center().gap_3()
        .child(div().text_sm().text_color(colors.muted_foreground).child(text.roundScore))
        .child(new Tag().variant(game.won ? "success" : "danger").outline().bg("#00000000").border_0().p_0()
          .h(40).text_3xl().font_bold().child(`${game.points > 0 ? "+" : ""}${game.points}`)))
      .child(button("ddz-deal", text.again, cx => view.start(cx)).primary().w_full().h(40)))
    .children(fireworks(view));
}

export function render(view, cx) {
  const header = Header.new("ddz-header", { content: JSON.stringify({ items: [
    { label: view.text.score, value: String(view.score), tone: view.score > 0 ? "success" : view.score < 0 ? "danger" : "default" },
    { label: view.text.multiplier, value: `×${view.game?.multiplier || 1}` },
  ], actions: [{ id: "ddz-reset-score", label: view.text.resetScore, icon: "icons/rotate-cw.svg" }] }) });
  if (!view.game) return lobby(view, cx).child(header);
  const { text, game } = view, colors = theme().colors;
  const compact = window.viewport_size().height < 680;
  const bodyHeight = Math.max(448, Math.min(788, window.viewport_size().height - 196));
  const yourTurn = game.turn === 0 && !view.busy && ["bid", "play"].includes(game.phase);
  const status = game.phase === "over" ? (game.won ? "win" : "lose") : game.phase === "redeal" ? "redeal"
    : view.busy ? "thinking" : game.turn !== 0 ? "paused" : game.phase === "bid" ? "bid" : "turn";
  const notice = div().id(`ddz-status-${status}`)
    .h_flex().flex_wrap().items_center().justify_center().gap_2().min_w(0).text_sm()
    .text_color(colors.muted_foreground)
    .children(view.busy ? [
      div().w(24).h(24).flex_shrink(0)
        .child(Image.new("ddz-status-avatar", { path: portrait(game.turn) })),
      new ShimmerText(text.pondering).id("ddz-thinking").text_sm(),
      div().id("ddz-elapsed").text_xs().child(`${view.elapsed} ${text.seconds}`),
    ] : game.phase === "redeal" ? [div().child(text[status])] : []);
  if (retryable.includes(view.error)) notice.child(IconButton.new("ddz-retry").rounded(9999).w(24).h(24).p_0()
    .tooltip(text.retry).accessibility_label(text.retry).on_click((_event, cx) => { cx.stop_propagation(); view.advance(cx); })
    .child(new Icon("icons/rotate-cw.svg").size("small")));
  const controls = div().id("ddz-controls").h_flex().flex_shrink(0).flex_wrap().justify_center().gap_2().min_w(208).min_h(32);
  if (game.phase === "redeal") controls.child(button("ddz-deal", text.again, cx => view.start(cx)).primary());
  if (yourTurn && game.phase === "bid") for (const amount of choices(game)) controls.child(
    button(`ddz-bid-${amount}`, amount ? `${amount} ${text.points}` : text.noBid, cx => view.act(amount, cx)).outline());
  if (yourTurn && game.phase === "play") controls
    .child(button("ddz-hint", text.hint, cx => { view.selected = legal(game.hands[0], game.last?.shape)[0] || []; view.hintPass = !view.selected.length && !!game.last; cx.notify(); }))
    .child(button("ddz-pass", text.pass, cx => view.act([], cx), view.hintPass).disabled(!game.last))
    .child(button("ddz-play", text.play, cx => view.act(view.selected, cx)).primary()
      .disabled(!beats(pattern(view.selected), game.last?.shape)));
  const played = tablePlay(game);
  const incoming = played && view.motion?.play === played && !view.motion.entered;
  const plays = div().id("ddz-plays").relative().v_flex().flex_1().min_h(208)
    .items_center().justify_center().gap_6().p_4()
    .child(div().id("ddz-last-play").relative().v_flex().w_full().min_h(compact ? 108 : 144)
      .items_center().justify_center().gap_3()
      .children(played ? [
        div().w(40).h(40).flex_shrink(0).accessibility_label(view.names[played.player])
          .child(Image.new(`ddz-play-avatar-${played.player}`, { path: portrait(played.player) })),
        tween(div().id("ddz-cards-motion").relative()).w_full().min_h(compact ? 64 : 92)
          .h_flex().items_center().justify_center().opacity(incoming ? 0 : 1).top(incoming ? 18 : 0)
          .child(cards(played.cards, cx, text, compact, !compact)),
      ] : [])
      .children(effects(view, cx)))
    .child(div().id("ddz-action-region").min_h(34).w_full().h_flex().items_center().justify_center()
      .child(notice.children(yourTurn || game.phase === "redeal" ? [controls] : [])));
  const info = div().id("ddz-info").w(200).h_full().flex_shrink(0).v_flex().border_l_1().border_color(colors.border)
    .child(div().v_flex().flex_shrink(0).gap_3().p_4().border_b_1().border_color(colors.border)
      .child(div().text_sm().font_semibold().child(text.bottom))
      .child(div().id(game.landlord === null ? "ddz-bottom-hidden" : "ddz-bottom-revealed")
        .child(game.phase !== "bid" && game.phase !== "redeal" ? cards(game.bottom, cx, text) : backs(3, cx, true))))
    .child(div().h_flex().flex_shrink(0).justify_between().p_4().border_b_1().border_color(colors.border)
      .child(div().text_xs().text_color(colors.muted_foreground).child(text.multiplier))
      .child(div().text_sm().font_semibold().child(`×${game.multiplier}`)))
    .child(history(view));
  const table = div().id(compact ? "ddz-felt-compact" : "ddz-felt").v_flex().w_full().max_w(1240).min_w(820)
    .child(div().id("ddz-player-strip").h_flex().h(89).flex_shrink(0).border_b_1().border_color(colors.border)
      .children([1, 0, 2].map((player, index) => seat(view, player, index === 2))))
    .child(div().h_flex().h(bodyHeight).flex_shrink(0).items_stretch()
      .child(div().v_flex().h_full().flex_1().min_w(0)
        .child(plays)
        .child(div().id("ddz-hand-region").v_flex().flex_shrink(0).items_center().gap_4().p_4().border_t_1().border_color(colors.border)
          .child(hand(view, cx, compact))))
      .child(info));
  return div().id("ddz-table").relative().size_full().v_flex().min_w(0).min_h(0).overflow_hidden()
    .text_color(colors.foreground).child(header).child(stage(table, 820))
    .children(game.phase === "over" && view.motion.resultReady ? [result(view, cx)] : []);
}
