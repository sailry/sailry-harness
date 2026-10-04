import { theme } from "sailry";
import { div } from "gpui-kit";
import { Button } from "gpui-base";
import { coordinate, SIZE } from "./game.js";

// GPUI Kit has no game-board control. The intersections, grid, and stones are
// game-specific material; ordinary actions and the surrounding UI use Kit controls.
export const material = { edge: "#b9bdb1", get line() { return theme().colors.border; },
  get star() { return theme().colors.muted_foreground; }, black: "#202024", white: "#fffaf0", last: "#cf593d" };

function line(cell, row, column, horizontal) {
  const first = horizontal ? column === 0 : row === 0;
  const last = horizontal ? column === SIZE - 1 : row === SIZE - 1;
  const start = first ? cell / 2 : 0;
  const length = first || last ? cell / 2 : cell;
  const element = div().absolute().bg(material.line);
  return horizontal ? element.top(cell / 2).left(start).w(length).h(1)
    : element.left(cell / 2).top(start).w(1).h(length);
}

function intersection(view, row, column, cell, canMove) {
  const { game, text } = view;
  const value = game.board[row][column];
  const last = game.moves.at(-1);
  const recent = last?.row === row && last?.column === column;
  const winning = game.winning.some(point => point.row === row && point.column === column);
  const entering = view.motion.stones.get(`${row}-${column}`)?.ready === false;
  const star = [3, 7, 11].includes(row) && [3, 7, 11].includes(column);
  const content = div().relative().size_full()
    .child(line(cell, row, column, true))
    .child(line(cell, row, column, false));
  if (star && !value) content.child(div().absolute().left(cell / 2 - 2).top(cell / 2 - 2)
    .w(4).h(4).rounded(9999).bg(material.star));
  if (value) {
    const piece = div().id(`gomoku-stone-${row}-${column}`)
    .absolute().left(cell * 0.13).top(cell * 0.13 - (entering ? 9 : 0))
    .w(cell * 0.74).h(cell * 0.74).opacity(entering ? 0.2 : 1)
    .transition("top", { duration: 200, easing: "ease-out" })
    .transition("opacity", { duration: 200, easing: "ease-out" })
    .rounded(9999).bg(value === 1 ? material.black : material.white)
    .border_color(winning ? material.last : value === 1 ? theme().colors.muted_foreground : material.edge)
    .shadow_sm().h_flex().items_center().justify_center()
    .children(recent ? [div().w(5).h(5).rounded(9999).bg(material.last)] : []);
    if (winning) piece.border_2(); else piece.border_1();
    content.child(piece);
    if (winning) {
      const visible = view.motion.result?.visible;
      content.child(div().id(`gomoku-winning-${row}-${column}`).absolute()
        .left(cell * (visible ? -0.06 : 0.13)).top(cell * (visible ? -0.06 : 0.13))
        .w(cell * (visible ? 1.12 : 0.74)).h(cell * (visible ? 1.12 : 0.74))
        .rounded(9999).border_2().border_color(material.last).opacity(visible ? 0 : 0.85)
        .transition("left", { duration: 450, easing: "ease-out" })
        .transition("top", { duration: 450, easing: "ease-out" })
        .transition("width", { duration: 450, easing: "ease-out" })
        .transition("height", { duration: 450, easing: "ease-out" })
        .transition("opacity", { duration: 450, easing: "ease-out" }));
    }
  }
  const label = text.cell(coordinate(row, column), value ? (value === 1 ? text.black : text.white) : text.empty)
    + (winning ? ` · ${text.winning}` : "");
  return Button.new(`gomoku-cell-${row}-${column}`).w(cell).h(cell).p_0().bg("#00000000")
    .accessibility_label(label).disabled(!canMove || value !== 0)
    .on_click((_event, cx) => { cx.stop_propagation(); view.act(row, column, cx); })
    .child(content);
}

export function board(view) {
  const height = window.viewport_size().height;
  const width = window.viewport_size().width;
  const cell = height < 760 || width < 1000 ? 24 : 30;
  const playable = view.game.phase === "play" && view.game.turn === view.game.human && !view.busy;
  const axis = div().h_flex().ml(22).w(cell * SIZE)
    .children(Array.from({ length: SIZE }, (_, column) => div().w(cell).text_center().text_xs()
      .text_color(theme().colors.muted_foreground).child(String.fromCharCode(65 + column))));
  const numbers = div().v_flex().w(22)
    .children(Array.from({ length: SIZE }, (_, row) => div().w(22).h(cell).h_flex()
      .items_center().justify_center().text_xs().text_color(theme().colors.muted_foreground).child(String(row + 1))));
  const grid = div().id("gomoku-board").v_flex().w(cell * SIZE).h(cell * SIZE)
    .flex_shrink(0)
    .children(view.game.board.map((_, row) => div().h_flex().h(cell)
      .children(Array.from({ length: SIZE }, (_, column) => intersection(view, row, column, cell, playable)))));
  return div().id("gomoku-board-wrap").v_flex().flex_shrink(0).items_center().gap_1()
    .child(axis)
    .child(div().h_flex().child(numbers).child(grid));
}
