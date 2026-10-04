import { div } from "gpui-kit";
import { Tag } from "gpui-component";

const pips = {
  1: [[1, 1]],
  2: [[0, 0], [2, 2]],
  3: [[0, 0], [1, 1], [2, 2]],
  4: [[0, 0], [0, 2], [2, 0], [2, 2]],
  5: [[0, 0], [0, 2], [1, 1], [2, 0], [2, 2]],
  6: [[0, 0], [0, 1], [0, 2], [2, 0], [2, 1], [2, 2]],
};

// Kit has no dice-face component. These pips are game material; ordinary
// actions, labels and status controls use Kit components in the view.
export function die(face, compact = false, hidden = false) {
  const size = compact ? 32 : 38;
  const dot = compact ? 5 : 6;
  const base = compact ? 5 : 6;
  const step = compact ? 8.5 : 10;
  const tile = div().relative().w(size).h(size).flex_shrink(0).rounded_md().border_1()
    .border_color(hidden ? "#86b6a4" : "#d9cfb5")
    .bg(hidden ? "#1a5260" : "#f8f2df").shadow_sm();
  if (hidden) return tile.h_flex().items_center().justify_center()
    .text_sm().font_bold().text_color("#cde5d9").child("?");
  return tile.children((pips[face] || []).map(([row, column]) =>
    div().absolute().left(base + column * step).top(base + row * step)
      .w(dot).h(dot).rounded(9999).bg("#364a42")));
}

function hand(view, player, compact) {
  const game = view.game;
  const rolling = view.motion?.rolling === true;
  const hidden = view.motion?.handShown === false || player !== 0
    && (game.phase === "bid" || game.result && (view.motion?.revealSeats ?? 2) < player);
  return div().id(`liars-hand-${player}`).h_flex().justify_center().gap(compact ? 4 : 6)
    .children(game.hands[player].map((face, index) => div().id(`liars-die-${player}-${index}`).relative()
      .transition("opacity", { duration: 210, easing: "ease-out" })
      .transition("top", { duration: 210, easing: "ease-out" })
      .opacity(rolling ? 0.45 : 1).top(rolling ? 14 : 0)
      .child(die(face, compact, hidden))));
}

export function seat(view, player, compact = false) {
  const { game, text } = view;
  const active = game.phase === "bid" && game.turn === player;
  const panel = div().id(`liars-seat-${player}`).v_flex().items_center().gap(compact ? 4 : 7)
    .min_w(compact ? 170 : 220).px(compact ? 10 : 16).py(compact ? 8 : 12)
    .rounded_xl().border_1().border_color(active ? "#e5c780" : "#83a996")
    .bg("#153d34").text_color("#f4f7ed").shadow_sm()
    .child(div().h_flex().items_center().gap_2()
      .child(div().text_sm().font_medium().child(view.names[player]))
      .child(new Tag().size("small").variant(game.counts[player] ? "secondary" : "danger")
        .child(game.counts[player] ? `${game.counts[player]} ${text.dice}` : text.eliminated)))
    .child(hand(view, player, compact));
  return panel;
}

export function table(view, compact = false) {
  const { game, text } = view;
  const current = game.currentBid;
  const bid = div().id("liars-current-bid").relative().v_flex().items_center().gap_1()
    .transition("top", { duration: 220, easing: "ease-out" })
    .top(view.motion?.bidPulse ? -4 : 0)
    .child(div().text_xs().text_color("#d2e4d5").child(text.currentBid))
    .child(current ? div().h_flex().items_center().gap_2()
      .child(div().text_2xl().font_bold().text_color("#f7df9c").child(`${current.quantity} ×`))
      .child(die(current.face, compact))
      .child(div().text_sm().text_color("#e7ede2").child(view.names[current.player]))
      : div().text_lg().text_color("#e7ede2").child(text.noBid));
  const center = div().v_flex().items_center().gap(compact ? 6 : 10)
    .child(bid)
    .child(div().max_w(420).text_center().text_xs().text_color("#c9dccd").child(text.rule));
  if (game.result) center.child(div().id("liars-round-result").relative().v_flex().items_center().gap_1()
    .transition("opacity", { duration: 240, easing: "ease-out" })
    .transition("top", { duration: 240, easing: "ease-out" })
    .opacity(view.motion?.resultShown === false ? 0 : 1)
    .top(view.motion?.resultShown === false ? 10 : 0)
    .child(div().text_sm().font_bold().text_color("#f7df9c")
      .child(game.result.actual >= game.result.quantity ? text.bidHeld : text.bluffCaught))
    .child(div().text_xs().text_color("#e7ede2")
      .child(`${text.counted(game.result.actual)} · ${text.loses(view.names[game.result.loser])}`)));
  const inside = div().id("liars-felt").v_flex().items_center().justify_between()
    .w_full().min_h(compact ? 370 : 450).p(compact ? 12 : 24).gap(compact ? 12 : 20)
    .rounded(compact ? 30 : 48).border_1().border_color("#ae9567").bg("#1a5943")
    .child(div().h_flex().items_start().justify_between().flex_wrap().w_full().gap_2()
      .children([1, 2].filter(player => view.matchEnabled[player]).map(player => seat(view, player, compact))))
    .child(center)
    .child(seat(view, 0, compact));
  return div().w_full().max_w(920).mx_auto().rounded(compact ? 36 : 55)
    .bg("#584336").p(compact ? 5 : 8).shadow_xl().child(inside);
}
