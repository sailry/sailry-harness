import { div } from "gpui-kit";
import { Button, Accordion, AccordionItem, Tag } from "gpui-component";
import { Image, theme } from "sailry";
import { Tooltip } from "sailry/ui";
import { netWorth } from "./game.js";
import { tileName, building, character, currencies, actionSheet } from "./art.js";

function amount(id, kind, label, value, text) {
  return div().id(id).h_flex().items_center().gap_1().min_w(0)
    .tooltip(label).accessibility_label(`${label} ${text.money(value)}`)
    .child(div().w(22).h(22).flex_shrink(0)
      .child(Image.new(`${id}-icon`, { path: currencies[kind] })))
    .child(div().text_sm().font_medium().truncate().child(text.money(value)));
}

export function seat(view, index) {
  const { game, text } = view, colors = theme().colors;
  const player = game.players[index], active = game.turn === index && game.phase !== "over";
  const delta = view.motion.cashDelta[index];
  return div().id(`city-seat-${index}`).relative().h(88).flex_1().min_w(0).px_4().py_3()
    .border_r(game.players.slice(index + 1).some(player => player.enabled) ? 1 : 0).border_color(colors.border)
    .child(div().h_flex().h_full().w_full().items_center().gap_3()
      .child(div().w(40).h(40).flex_shrink(0).child(Image.new(`city-avatar-${index}`, { path: character(view.characters[index], true) })))
      .child(div().v_flex().flex_1().min_w(0).gap_1()
        .child(div().h(22).h_flex().items_center().gap_2()
          .child(div().truncate().text_sm().font_semibold().child(view.names[index]))
          .children(active ? [new Tag().variant("secondary").size("small").child(text.acting)] : [])
          .children(player.bankrupt ? [div().text_xs().text_color(colors.muted_foreground).child(text.bankrupted)] : []))
        .child(div().h_flex().items_center().gap_4()
          .child(amount(`city-balance-${index}-${player.cash}`, "cash", text.cash, player.cash, text))
          .child(amount(`city-worth-${index}`, "worth", text.netWorth, netWorth(game, index), text))))
      .children(delta ? [div().id(`city-cash-${index}`).absolute().right(0).top(delta.visible ? -10 : 0)
        .opacity(delta.visible ? 1 : 0).transition("top", { duration: 320, easing: "ease-out" })
        .transition("opacity", { duration: 240, easing: "ease-out" }).text_sm().font_medium()
        .text_color(delta.amount > 0 ? colors.success : colors.destructive)
        .child(`${delta.amount > 0 ? "+" : ""}${text.money(delta.amount)}`)] : []))
    .children(active ? [div().absolute().left(16).right(16).bottom(0).h(2).bg(colors.ring)] : []);
}

export function portfolio(view, height) {
  const { game, text } = view, colors = theme().colors;
  const seats = game.players.map((player, index) => player.enabled ? index : null).filter(index => index !== null);
  const accordion = new Accordion("city-portfolios").multiple(true).bordered(false).size("small").h_auto()
    .on_toggle((indices, cx) => { view.expanded = indices.map(index => seats[index]); cx.notify(); });
  for (const seat of seats) {
    const owned = game.board.filter(tile => tile.kind === "property" && game.properties[tile.index].owner === seat);
    const section = new AccordionItem().open(view.expanded.includes(seat)).bg("#00000000")
      .title(div().h_flex().items_center().gap_2().w_full()
        .child(div().w(28).h(28).child(Image.new(`city-portfolio-avatar-${seat}`, { path: character(view.characters[seat], true) })))
        .child(div().flex_1().truncate().text_sm().font_medium().child(view.names[seat]))
        .child(div().text_xs().text_color(colors.muted_foreground).child(String(owned.length))));
    const body = div().id(`city-portfolio-${seat}-${view.expanded.includes(seat) ? "open" : "closed"}`).v_flex().gap_1().py_1();
    if (!owned.length) body.child(div().text_xs().text_color(colors.muted_foreground).child(text.emptyProperties));
    for (const tile of owned) {
      const value = tile.price + game.properties[tile.index].level * tile.upgrade;
      body.child(new Button(`city-property-${tile.index}`).ghost().w_full().h(32).px_1().py_0()
        .on_click((_event, cx) => view.inspect(tile.index, cx))
        .child(div().h_flex().items_center().gap_2().w_full()
          .child(div().w(30).h(30).flex_shrink(0).child(Image.new(`city-list-art-${tile.index}`, { path: building(tile) })))
          .child(div().flex_1().min_w(0).text_xs().truncate().child(tileName(text, tile)))
          .child(div().h_flex().items_center().gap_1().text_xs().text_color(colors.muted_foreground)
            .child(div().w(16).h(16).flex_shrink(0).child(Image.new(`city-list-coin-${tile.index}`, { path: currencies.cash })))
            .child(text.money(value)))));
    }
    accordion.child(section.child(body));
  }
  return div().id("city-all-assets").v_flex().gap_3().max_h(height)
    .child(div().text_sm().font_semibold().child(text.allAssets))
    .child(div().max_h(height - 36).overflow_y_scroll().child(accordion));
}

export const ASSETS_HEIGHT = 176;

export function assets(view) {
  const { game, text } = view, colors = theme().colors;
  const cash = game.players[0].cash, worth = netWorth(game, 0);
  return div().id("city-my-assets").h(ASSETS_HEIGHT).v_flex().gap_4().px_4().py_4()
    .border_b_1().border_color(colors.border)
    .child(div().text_sm().font_semibold().child(text.myAssets))
    .child(div().h_flex().items_center().justify_between().gap_2()
      .child(div().text_xs().text_color(colors.muted_foreground).child(text.cash))
      .child(amount("city-assets-cash", "cash", text.cash, cash, text)))
    .child(div().h_flex().items_center().justify_between().gap_2().text_sm()
      .child(div().text_xs().text_color(colors.muted_foreground).child(text.propertyValue))
      .child(text.money(worth - cash)))
    .child(div().h_flex().items_center().justify_between().gap_2()
      .child(div().text_xs().text_color(colors.muted_foreground).child(text.netWorth))
      .child(amount("city-assets-worth", "worth", text.netWorth, worth, text)));
}

export const AUCTION_HEIGHT = 192;

export function auction(view) {
  const { game, text } = view, colors = theme().colors;
  const sale = game.auction, tile = game.board[sale.tile];
  return div().id("city-auction").h(AUCTION_HEIGHT).v_flex().gap_3().p_4()
    .border_b_1().border_color(colors.border)
      .child(div().text_sm().font_semibold().child(text.phase.auction))
      .child(new Button("city-auction-property").ghost().w_full().h(48).p_0()
        .on_click((_event, cx) => view.inspect(sale.tile, cx))
        .child(div().h_flex().items_center().gap_2().w_full()
          .child(div().w(44).h(44).flex_shrink(0)
            .child(Image.new("city-auction-art", { path: building(tile) })))
          .child(div().v_flex().gap_1().min_w(0).flex_1()
            .child(div().text_sm().font_medium().truncate().child(tileName(text, tile)))
            .child(div().text_xs().text_color(colors.muted_foreground).child(text.groups[tile.group])))))
      .child(div().h_flex().items_center().justify_between().gap_2().text_xs()
        .child(div().text_color(colors.muted_foreground).child(text.bidNow))
        .child(div().id("city-auction-price").text_lg().font_semibold()
          .child(sale.leader === null ? text.noBid : text.money(sale.bid))))
      .child(div().h_flex().items_center().justify_between().gap_2().text_xs()
        .child(div().text_color(colors.muted_foreground).child(text.highBidder))
        .child(div().id(`city-auction-leader-${sale.leader}`).min_w(0).truncate()
          .child(sale.leader === null ? text.noBid : view.names[sale.leader])));
}

export function activity(view, height) {
  const { game, text } = view, colors = theme().colors;
  return div().id("city-activity").v_flex().gap_4().h(height).p_4()
      .child(div().text_sm().font_semibold().child(text.latest))
      .child(div().id("city-history").v_flex().gap_3().flex_1().min_h(0).overflow_y_scroll()
        .children(game.history.filter(entry => entry.key !== "end").slice(-40).reverse().map((entry, index) => {
          const item = activityItem(view, entry);
          return div().id(`city-event-${index}`).h_flex().items_center().gap_2()
            .child(div().id(`city-history-action-${index}`).w(32).h(32).flex_shrink(0).relative().overflow_hidden()
              .accessibility_label(item.label)
              .child(Tooltip.new(`city-history-tooltip-${index}`, { text: item.label })
                .child(div().absolute().left(-(item.icon % 4) * 32).top(-Math.floor(item.icon / 4) * 32).w(128).h(96)
                  .child(Image.new(`city-history-symbol-${index}`, { path: actionSheet })))))
            .child(div().v_flex().gap_0().min_w(0).flex_1().text_xs().line_height(1.25)
              .child(div().text_color(colors.foreground).truncate().child(item.title))
              .children(item.description ? [div().h_flex().items_center().gap_1()
                .text_color(item.flow > 0 ? colors.success : item.flow < 0 ? colors.destructive : colors.muted_foreground)
                .child(div().min_w(0).truncate().child(item.description))] : []))
            .child(div().w(28).h(28).flex_shrink(0).child(Image.new(`city-history-avatar-${index}`, {
                path: character(view.characters[entry.seat ?? 0], true),
              })));
        })));
}

const activityIcons = {
  roll: 0, passed_start: 10, buy: 3, rent: 2,
  bonus: 1, tax: 2, event: 1, debt: 2, debt_paid: 9,
  decline: 7, auction_started: 7, bid: 7, pass: 7,
  auction_won: 7, auction_unsold: 7, end: 10,
  upgrade: 8, sell: 9, trade_proposed: 4,
  trade_accepted: 5, trade_rejected: 6, bankrupt: 6, finish: 11,
};

export function activityItem(view, entry) {
  const { game, text } = view;
  const auction = ["decline", "auction_started", "bid", "pass", "auction_won", "auction_unsold"].includes(entry.key);
  const label = auction ? text.phase.auction : text.events[entry.key] || text.kinds.event;
  const title = entry.key === "event" ? text.cityEvents[entry.event]
    : Number.isInteger(entry.tile) ? tileName(text, game.board[entry.tile]) : label;
  const amount = entry.amount ?? entry.price;
  const debit = ["buy", "rent", "tax", "upgrade", "auction_won", "debt_paid"].includes(entry.key);
  const credit = ["bonus", "passed_start", "sell", "trade_accepted"].includes(entry.key);
  const flow = Number.isFinite(amount) ? debit ? -Math.abs(amount) : credit || entry.key === "event" ? amount : 0 : 0;
  const description = entry.dice ? text.steps(entry.dice.reduce((total, value) => total + value, 0)) : Number.isFinite(amount)
    ? text.money(Math.abs(amount)) : entry.key === "pass" ? text.action.pass
      : Number.isInteger(entry.tile) ? text.events[entry.key] : null;
  const icon = entry.key === "event" ? amount > 0 ? 1 : 2 : activityIcons[entry.key] ?? 1;
  return { title: title || label, label, description, icon, flow, currency: !entry.dice && Number.isFinite(amount) };
}
