import { div } from "gpui-kit";
import { Tag } from "gpui-component";
import { Image, theme } from "sailry";
import { portrait } from "./layout.js";
import { ranks, rank, red, suits } from "./cards.js";

// Match the Dou Dizhu card paper, ink, dimensions and type placement.
export function card(value, compact = false) {
  const content = div().v_flex().size_full().p(compact ? 2 : 4)
    .text_color(red(value) ? "#b34343" : "#25342f")
    .child(div().h_flex().items_start()
      .child((compact ? div().text_sm() : div().text_base()).font_bold().line_height(1)
        .child(ranks[rank(value) - 2])))
    .child((compact ? div().text_lg() : div().text_2xl())
      .flex_1().h_flex().items_center().justify_center().child(suits[value % 4]));
  return div().w(compact ? 44 : 64).h(compact ? 64 : 92).flex_shrink(0).rounded_md()
    .border_1().border_color("#e5dcc9").bg("#fffdf6").shadow_md().child(content);
}

export function back(index, compact = false) {
  return div().w(compact ? 44 : 64).h(compact ? 64 : 92).flex_shrink(0)
    .rounded_md().border_1().border_color("#bca47a").bg("#21475f").shadow_md()
    .child(Image.new(`poker-back-${index}`, { path: "dev.sailry.platform/desktop/card-back.svg" }));
}

function empty(compact) {
  return div().w(compact ? 44 : 64).h(compact ? 64 : 92).flex_shrink(0)
    .rounded_md().border_1().border_color(theme().colors.border);
}

export function cards(values, count = values.length, compact = false, hidden = false) {
  return div().h_flex().justify_center().gap(compact ? 4 : 8)
    .children(Array.from({ length: count }, (_, index) =>
      index < values.length ? hidden ? back(index, compact) : card(values[index], compact) : empty(compact)));
}

export function seat(view, player) {
  const { game, text } = view, colors = theme().colors;
  const active = game.phase === "betting" && game.turn === player;
  return div().id(`poker-seat-${player}`).relative().h(88).flex_1().min_w(0).px_4().py_3()
    .border_r(player === 0 ? 1 : 0).border_color(colors.border)
    .child(div().h_flex().h_full().items_center().gap_3()
      .child(div().w(40).h(40).flex_shrink(0)
        .child(Image.new(`poker-avatar-${player}`, { path: portrait(player) })))
      .child(div().v_flex().flex_1().min_w(0).gap_1()
        .child(div().h_flex().items_center().gap_2()
          .child(div().text_sm().font_semibold().truncate().child(view.names[player]))
          .children(game.dealer === player ? [new Tag().size("small").variant("secondary").child(text.dealer)] : []))
        .child(div().h_flex().flex_wrap().gap_3().text_xs().text_color(colors.muted_foreground)
          .child(div().child(`${text.chips} ${game.stacks[player]}`))
          .child(div().child(`${text.bet} ${game.streetBets[player]}`))))
      .children(player === 1 ? [hole(view, 1, true)] : []))
    .children(active ? [div().id(`poker-active-${player}`).absolute().left(16).right(16).bottom(0).h(2).bg(colors.ring)] : []);
}

export function hole(view, player, compact) {
  const { game } = view;
  const reveal = game.phase === "over" && game.settlement?.reason === "showdown";
  return div().v_flex().items_center().gap_2()
    .child(div().id(`poker-hole-${player}`).relative()
      .transition("opacity", { duration: 220, easing: "ease-out" })
      .transition("top", { duration: 220, easing: "ease-out" })
      .opacity(view.motion?.holeVisible === false ? 0 : 1)
      .top(view.motion?.holeVisible === false ? 16 : 0)
      .child(cards(game.hands[player], 2, compact, player === 1 && !reveal)));
}

export function felt(view, compact, actions) {
  const { game, text } = view, colors = theme().colors;
  const shown = view.motion?.boardVisible ?? game.shown;
  const street = game.phase === "over"
    ? game.settlement.reason === "showdown" ? text.showdown : text.handComplete
    : text[game.stage];
  const community = div().id("poker-community").v_flex().items_center().justify_center().gap_3().min_h(128).w_full()
    .child(div().h_flex().flex_wrap().items_center().justify_center().gap_2().text_xs()
      .child(div().font_medium().child(text.community))
      .child(div().text_color(colors.muted_foreground).child(street)))
    .child(div().h_flex().justify_center().gap(compact ? 4 : 8)
      .children(Array.from({ length: 5 }, (_, index) => {
        const open = index < game.shown;
        return div().id(`poker-board-${index}`).relative()
          .transition("opacity", { duration: 220, easing: "ease-out" })
          .transition("top", { duration: 220, easing: "ease-out" })
          .opacity(open && index >= shown ? 0 : 1)
          .top(open && index >= shown ? 12 : 0)
          .child(open ? card(game.board[index], compact) : empty(compact));
      })));
  return div().id("poker-felt").v_flex().flex_1().min_h(300).items_center().justify_center().w_full().p_4().gap_6()
    .child(community).child(actions);
}
