import { div } from "gpui-kit";
import { Button } from "gpui-base";
import { Image, theme } from "sailry";
import { Tag } from "gpui-component";
import { portrait } from "./layout.js";
import { label, red } from "./cards.js";

// Card paper and ink retain their game materials; the table uses the Kit theme.
export const tableMaterials = {
  trim: "#bca47a",
  paper: "#fffdf6",
  paperBorder: "#e5dcc9",
  ink: "#25342f",
  red: "#b34343",
  back: "#21475f",
};

export function card(value, cx, text, selected = false, click = null, small = false, compact = false) {
  const { colors } = theme();
  const fill = tableMaterials.paper;
  const ink = red(value) ? tableMaterials.red : tableMaterials.ink;
  const suit = value >= 52 ? (value === 52 ? "♛" : "♚") : ["♠", "♥", "♣", "♦"][value % 4];
  const rankStr = value >= 52
    ? "JK"
    : ["3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A", "2"][Math.floor(value / 4)];
  const face = value >= 52 ? (value === 52 ? text.smallJoker : text.bigJoker) : label(value);

  const rankEl = div().font_bold().line_height(1);
  if (small) rankEl.text_sm();
  else rankEl.text_base();

  const symbolEl = div().flex_1().h_flex().items_center().justify_center();
  if (small) symbolEl.text_lg();
  else symbolEl.text_2xl();

  const content = div().v_flex().size_full().p(small ? 2 : 4).text_color(ink)
    .child(div().h_flex().items_start().child(rankEl.child(rankStr)))
    .child(symbolEl.child(suit));

  const width = small ? 44 : 64, height = small ? 64 : 92;
  if (click) return Button.new(`card-${value}`).w(width).h(height).p_0().rounded_md().bg(fill)
    .border_1().border_color(selected ? colors.ring : tableMaterials.paperBorder).shadow_md()
    .accessibility_label(face).on_click(click).child(content);
  return div().w(width).h(height).flex_shrink(0).rounded_md().border_1().border_color(tableMaterials.paperBorder)
    .bg(fill).shadow_md().child(content);
}

export function cards(values, cx, text, overlap = false, large = false) {
  if (large) return div().h_flex().w_full().max_w(values.length * 68).pr(32)
    .children(values.map(value => div().relative().flex_1().min_w_0().h(92)
      .child(div().absolute().left_0().top_0().child(card(value, cx, text)))));
  if (overlap && values.length > 3) return div().h_flex().w_full().max_w(values.length * 48).pr(12)
    .children(values.map(value => div().relative().flex_1().min_w_0().h(64)
      .child(div().absolute().left_0().top_0().child(card(value, cx, text, false, null, true)))));
  return div().h_flex().flex_wrap().justify_center().gap_1().children(values.map(value => card(value, cx, text, false, null, true)));
}

export function backs(count, cx, spread = false, step = 5) {
  return div().h_flex().pr(spread ? 0 : 44 - step).children(Array.from({ length: count }, (_, index) =>
    div().relative().w(spread ? 48 : step).h(64).child(div().absolute().left_0().top_0().w(44).h(64)
      .rounded_md().border_1().border_color(tableMaterials.trim).bg(tableMaterials.back).shadow_md()
      .child(Image.new(`card-back-${index}`, { path: "dev.sailry.platform/desktop/card-back.svg" })))));
}

export function seat(view, player, last = false) {
  const { game, text } = view, colors = theme().colors;
  const active = ["bid", "play"].includes(game.phase) && game.turn === player;
  return div().id(`ddz-seat-${player}`).relative().h(88).flex_1().min_w(0).px_4().py_3()
    .border_r(last ? 0 : 1).border_color(colors.border)
    .child(div().h_flex().h_full().items_center().gap_3()
      .child(div().w(40).h(40).flex_shrink(0)
        .child(Image.new(`ddz-avatar-${player}`, { path: portrait(player) })))
      .child(div().v_flex().flex_1().min_w(0).gap_1()
        .child(div().h_flex().items_center().gap_2()
          .child(div().text_sm().font_semibold().truncate().child(view.names[player]))
          .children(game.landlord === player ? [new Tag().variant("secondary").size("small").child(text.landlord)] : [])
          .children(active ? [new Tag().variant("secondary").size("small").child(text.acting)] : []))
        .child(div().text_xs().text_color(colors.muted_foreground).truncate()
          .child(`${game.hands[player].length} ${text.cards}`))))
    .children(active ? [div().id(`ddz-active-${player}`).absolute().left(16).right(16).bottom(0).h(2).bg(colors.ring)] : []);
}
