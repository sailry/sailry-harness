import { div } from "gpui-kit";
import { Button } from "gpui-component";
import { Image, theme } from "sailry";
import { Tooltip } from "sailry/ui";
import { choices } from "./game.js";
import { tileName, building, character, diceImage, diceShadow } from "./art.js";
import { action, button, status } from "./controls.js";
import { decisionTile } from "./details.js";
import { MAP_SIZE } from "./map.js";

// The perimeter layout follows the illustrated board from f1e7403e. Kit has no
// board control; compose its buttons and image host with the shared theme.
function tile(view, item, cell) {
  const { game, text } = view, colors = theme().colors;
  const { column, row } = item, holding = game.properties[item.index];
  const highlighted = view.motion.highlight?.tile === item.index;
  const occupied = game.players.some((player, seat) => player.enabled && !player.bankrupt && view.motion.positions[seat] === item.index);
  const artSize = Math.min(60, cell - 50);
  const content = div().id(`city-tile-${item.index}`).relative().size_full().overflow_hidden()
    .child(div().absolute().top(0).left((cell - 16 - artSize) / 2).w(artSize).h(artSize).overflow_hidden()
      .child(Image.new(`city-building-art-${item.index}`, { path: building(item) })))
    .child(div().id(`city-tile-label-${item.index}`).absolute().bottom(17).left(0).w_full().h(16)
      .text_xs().font_medium().text_center().truncate().child(tileName(text, item)))
    .child(div().absolute().bottom(0).left(0).w_full().h(16).h_flex().items_center().justify_center()
      .gap_1().text_xs().text_color(colors.muted_foreground)
      .child(item.kind === "property" ? text.money(item.price) : item.amount ? text.money(item.amount) : text.kinds[item.kind]));
  if (holding.owner !== null) content.child(div().id(`city-owner-${item.index}`).absolute().right(0).top(0)
    .w(18).h(18).accessibility_label(view.names[holding.owner])
    .child(Tooltip.new(`city-owner-tooltip-${item.index}`, { text: view.names[holding.owner] })
      .child(Image.new(`city-owner-avatar-${item.index}`, { path: character(view.characters[holding.owner], true) }))));
  if (holding.level) content.child(div().id(`city-building-${item.index}`).absolute().left(3).bottom(32)
    .text_xs().text_color(colors.foreground).child(`+${holding.level}`));
  return new Button(`city-cell-${item.index}`).outline().absolute().left(column * cell + 3).top(row * cell + 3)
    .w(cell - 6).h(cell - 6).p_1().rounded_lg().bg(occupied ? colors.accent : colors.surface)
    .border_color(highlighted ? colors.ring : colors.border)
    .tooltip(`${tileName(text, item)}, ${item.kind === "property" ? text.money(item.price) : text.kinds[item.kind]}`)
    .on_click((_event, cx) => view.inspect(item.index, cx)).child(content);
}

function token(view, seat, cell) {
  const index = view.motion.positions[seat], { column, row } = view.game.board[index];
  const occupants = view.game.players.map((player, i) => player.enabled && !player.bankrupt && view.motion.positions[i] === index ? i : null).filter(i => i !== null);
  const { x, y } = tokenPosition(column, row, cell, occupants.indexOf(seat), occupants.length);
  return div().id(`city-token-${seat}`).absolute().left(x).top(y).w(34).h(46)
    .transition("left", { duration: 125, easing: "ease-out" })
    .transition("top", { duration: 125, easing: "ease-out" })
    .child(div().size_full().child(Image.new(`city-token-art-${seat}`, { path: character(view.characters[seat]) })));
}

// Keep each group inside the open board area, including the start corners.
export function tokenPosition(column, row, cell, occupant, count) {
  const side = MAP_SIZE - 1, inset = 14, width = 34, height = 46;
  const horizontal = row === 0 || row === side;
  const step = horizontal ? 34 : 44;
  const spread = (count - 1) * step;
  const low = cell + inset, high = cell * side - inset;
  const clamp = (value, end) => Math.max(low, Math.min(value, end));
  const x = horizontal
    ? clamp(column * cell + cell / 2 - width / 2 - spread / 2, high - width - spread) + occupant * step
    : column === 0 ? low : high - width;
  const y = horizontal ? row === 0 ? low : high - height
    : clamp(row * cell + cell / 2 - height / 2 - spread / 2, high - height - spread) + occupant * step;
  return { x, y };
}

function die(view, index) {
  const motion = view.motion;
  const frame = !motion.rolling ? null : motion.settling ? motion.diceFrame
    : (motion.diceFrame + index * 7) % 20;
  return div().id(`city-die-${index}-${motion.dice[index]}`).relative().w(88).h(94)
    .child(div().absolute().left(5).bottom(0).w(78).h(29).opacity(0.45)
      .child(Image.new(`city-die-shadow-${index}`, { path: diceShadow })))
    .child(div().absolute().left(0).top(motion.rolling && !motion.settling && motion.diceFrame % 4 < 2 ? -5 : 0).w(88).h(88)
      .transition("top", { duration: 70, easing: "ease-out" })
      .child(Image.new(`city-die-art-${index}`, { path: diceImage(motion.dice[index], frame, motion.settling) })));
}

function center(view, cell) {
  const { text, game } = view;
  const box = div().id("city-board-center").absolute().left(cell + 44).top(cell + 44)
    .w(cell * (MAP_SIZE - 2) - 88).h(cell * (MAP_SIZE - 2) - 88)
    .v_flex().items_center().justify_center().gap_4();
  if (game.phase === "over") return box
    .child(div().w(100).h(100).child(Image.new("city-finished-art", { path: building({ kind: "bonus" }) })))
    .child(div().text_lg().font_semibold().child(text.phase.over))
    .child(button("city-open-result", text.result, cx => view.showResult(cx), false, true));
  box.child(div().id("city-dice").h_flex().gap_1().children([die(view, 0), die(view, 1)]));
  const actions = div().v_flex().min_w(0).items_center().gap_2().child(status(view));
  const move = game.turn === 0 ? choices(game).find(move => move.kind === "roll") : null;
  if (view.animating) actions.child(div().h(34));
  else if (move) actions.child(action(view, move, 0, true, true));
  else if (game.turn === 0) actions.child(button("city-open-decision", text.phase[game.phase], cx => view.inspect(decisionTile(view), cx), view.animating));
  return box.child(actions);
}

export function board(view, cell) {
  return div().id("city-board").relative().w(cell * MAP_SIZE).h(cell * MAP_SIZE).flex_shrink(0)
    .child(center(view, cell))
    .children(view.game.board.map(item => tile(view, item, cell)))
    .children(view.game.players.map((player, seat) => !player.enabled || player.bankrupt ? null : token(view, seat, cell)).filter(Boolean));
}
