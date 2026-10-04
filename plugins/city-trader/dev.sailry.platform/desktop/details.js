import { div } from "gpui-kit";
import { Image, theme } from "sailry";
import { choices, rent } from "./game.js";
import { tileName, building, character, currencies } from "./art.js";
import { action, button } from "./controls.js";

export function decisionTile(view) {
  const game = view.game;
  return game.auction?.tile ?? game.trade?.tile ?? game.debt?.tile
    ?? (game.phase === "buy" ? game.players[game.active].position : view.selected)
    ?? game.players[0].position;
}

export function details(view) {
  const { game, text } = view, colors = theme().colors;
  const index = view.selected, tile = game.board[index], holding = game.properties[index];
  const property = tile.kind === "property";
  const auction = game.auction?.tile === index ? game.auction : null;
  const trade = game.trade?.tile === index ? game.trade : null;
  const debt = game.phase === "debt" && game.turn === 0;
  const decision = game.turn === 0 && !view.animating
    && (debt || index === decisionTile(view) && ["buy", "auction", "trade"].includes(game.phase));
  const participant = trade ? game.active : auction?.leader;
  const box = div().id("city-property-dialog").w(400).max_h(Math.max(300, window.viewport_size().height - 100))
    .v_flex().gap_3().overflow_y_scroll()
    .child(div().h_flex().justify_between().items_center().gap_3()
      .child(div().h_flex().items_center().gap_4().flex_1().min_w(0)
        .child(div().text_sm().text_color(colors.muted_foreground)
          .child(decision ? text.phase[game.phase] : property ? text.groups[tile.group] : text.kinds[tile.kind]))
        .children(participant !== undefined && participant !== null ? [
          div().id("city-detail-participant").h_flex().items_center().gap_2().text_sm().min_w(0)
            .child(div().w(24).h(24).flex_shrink(0)
              .child(Image.new("city-detail-avatar", { path: character(view.characters[participant], true) })))
            .child(div().min_w(0).truncate().child(view.names[participant])),
        ] : []))
      .child(button("city-close-details", text.close, cx => view.closeDetails(cx), false, false, "icons/close.svg")))
    .child(div().id(`city-selected-${index}`).v_flex().items_center().gap_3().text_center()
      .child(div().w(104).h(104).flex_shrink(0)
        .child(Image.new("city-detail-art", { path: building(tile) })))
      .child(div().v_flex().items_center().gap_1().w_full()
        .child(div().text_xl().font_semibold().child(tileName(text, tile)))
        .children(!property && text.specials[tile.kind] ? [div().text_sm().text_color(colors.muted_foreground).child(text.specials[tile.kind])] : [])));

  const priceLabel = debt ? text.debtDue : null;
  const price = debt ? text.money(game.debt.amount) : trade ? text.money(trade.price)
    : auction ? auction.leader === null ? text.noBid : text.money(auction.bid)
    : property ? text.money(tile.price) : tile.amount ? `${tile.kind === "bonus" ? "+" : "-"}${text.money(tile.amount)}` : null;
  const summary = div().id("city-detail-summary").v_flex().gap_2()
    .children(priceLabel ? [div().text_center().text_xs().text_color(colors.muted_foreground).child(priceLabel)] : [])
    .children(price ? [div().id("city-detail-price").h_flex().items_center().justify_center().gap_2().text_2xl().font_semibold()
      .text_color(debt ? colors.destructive : colors.foreground)
      .children(auction?.leader === null ? [] : [div().w(28).h(28).flex_shrink(0)
        .child(Image.new("city-detail-coin", { path: currencies.cash }))])
      .child(price)] : []);
  if (price) box.child(summary);

  if (property) box.child(div().h_flex().gap_3().pt_3().border_t_1().border_color(colors.border).children([
    [text.rent, text.money(rent(game, index)), true], [text.level, String(holding.level), false],
    [text.upgradeCost, text.money(tile.upgrade), true],
  ].map(([label, value, currency], index) => div().flex_1().min_w(0).v_flex().items_center().text_center().gap_1()
    .child(div().text_xs().text_color(colors.muted_foreground).child(label))
    .child(div().h_flex().items_center().justify_center().gap_1().text_sm().font_medium()
      .children(currency ? [div().w(16).h(16).flex_shrink(0)
        .child(Image.new(`city-detail-stat-coin-${index}`, { path: currencies.cash }))] : [])
      .child(value)))));
  if (debt) {
    box.child(div().h_flex().flex_wrap().gap_2().children(game.board.filter(tile => game.properties[tile.index].owner === 0).map(tile =>
      button(`city-debt-property-${tile.index}`, tileName(text, tile), cx => { view.selected = tile.index; cx.notify(); }, false, index === tile.index))));
  }
  const moves = game.turn === 0 && game.phase !== "over" ? choices(game).filter(move =>
    Number.isInteger(move.tile) ? move.tile === index
      : !["roll", "end"].includes(move.kind) && (game.phase === "debt" || index === decisionTile(view))) : [];
  if (moves.some(move => move.kind === "propose")) box.child(div().text_xs()
    .text_color(colors.muted_foreground).child(text.offerPrices));
  if (moves.length) {
    const controls = moves.map((move, i) => {
      const first = moves.findIndex(other => other.kind === move.kind) === i;
      return action(view, move, i, first, first && ["buy", "bid", "accept", "upgrade"].includes(move.kind), true);
    });
    box.child(div().id("city-action-list").v_flex().gap_2().pt_2()
      .children(Array.from({ length: Math.ceil(controls.length / 2) }, (_, row) =>
        div().h_flex().w_full().gap_2().children(controls.slice(row * 2, row * 2 + 2)))));
  }

  return box;
}

export function eventDetails(view) {
  const { feedback, text } = view, { entry, amount, due, visible } = feedback;
  const colors = theme().colors;
  const reason = entry.key === "event" ? text.cityEvents[entry.event] : text.events[entry.key];
  return div().id("city-event-dialog").w(320).v_flex().gap_2()
    .child(div().id("city-event-content").relative().top(visible ? 0 : 12).opacity(visible ? 1 : 0)
      .transition("top", { duration: 240, easing: "ease-out" })
      .transition("opacity", { duration: 240, easing: "ease-out" })
      .v_flex().items_center().gap_3().pb_4().text_center()
      .child(div().w(88).h(88).child(Image.new("city-event-coin", { path: currencies.cash })))
      .child(div().id("city-event-amount").text_3xl().font_bold()
        .text_color(amount < 0 ? colors.destructive : colors.foreground)
        .child(due ? text.money(Math.abs(amount)) : `${amount > 0 ? "+" : ""}${text.money(amount)}`))
      .child(div().h_flex().items_center().justify_center().gap_2().text_sm().text_color(colors.muted_foreground)
        .child(div().w(24).h(24).flex_shrink(0).accessibility_label(view.names[entry.seat])
          .child(Image.new("city-event-avatar", { path: character(view.characters[entry.seat], true) })))
        .child(div().v_flex().items_start()
          .children(due ? [div().child(text.debtDue)] : [])
          .child(div().child(reason)))));
}
