import { theme } from "sailry";
import { div } from "gpui-kit";
import { Button } from "gpui-base";
import { choices, coordinate, SIZE } from "./game.js";

// Kit has no Reversi board. Only board squares, legal markers, and discs use
// game material colors; the surrounding layout and actions use Kit and theme.
export const material = { get line() { return theme().colors.border; },
  get hint() { return theme().colors.muted_foreground; }, black: "#202327", white: "#fffaf0",
  whiteEdge: "#b9bdb1", last: "#d3a14e" };

function cell(view, row, column, size, legal, last) {
  const { game, text } = view;
  const value = game.board[row][column];
  const recent = last?.row === row && last?.column === column;
  const motion = view.motion.discs.get(`${row}-${column}`);
  const color = motion && !motion.placed && motion.phase < 2 ? motion.from : value;
  const entering = motion?.placed && motion.phase === 0;
  const flipping = motion && !motion.placed && motion.phase === 1;
  const content = div().relative().size_full().h_flex().items_center().justify_center();
  if (value) content.child(div().id(`reversi-disc-${row}-${column}`)
    .relative().top(entering ? -8 : 0).opacity(entering ? 0.3 : 1)
    .w(size * (flipping ? 0.08 : 0.74)).h(size * 0.74).rounded(9999).shadow_md()
    .transition("width", { duration: 110, easing: "ease-in-out" })
    .transition("top", { duration: 180, easing: "ease-out" })
    .transition("opacity", { duration: 180, easing: "ease-out" })
    .bg(color === 1 ? material.black : material.white)
    .border_1().border_color(color === 1 ? theme().colors.muted_foreground : material.whiteEdge)
    .h_flex().items_center().justify_center()
    .children(recent ? [div().w(6).h(6).rounded(9999).bg(material.last)] : []));
  else if (legal) content.child(div().id(`reversi-legal-${row}-${column}`)
    .w(10).h(10).rounded(9999).bg(material.hint));
  const label = text.cell(coordinate(row, column), value ? (value === 1 ? text.black : text.white) : text.empty)
    + (legal ? ` · ${text.legal}` : "");
  return Button.new(`reversi-cell-${row}-${column}`).w(size).h(size).p_0()
    .bg("#00000000")
    .border_1().border_color(material.line)
    .accessibility_label(label).disabled(!legal)
    .on_click((_event, cx) => { cx.stop_propagation(); view.act(row, column, cx); })
    .child(content);
}

export function board(view) {
  const height = window.viewport_size().height;
  const width = window.viewport_size().width;
  const size = height < 760 || width < 1000 ? 42 : 54;
  const playable = view.game.phase === "play" && view.game.turn === view.game.human && !view.busy;
  const offered = new Set((playable ? choices(view.game) : [])
    .map(move => move.row * SIZE + move.column));
  const last = view.game.history.findLast(entry => !entry.pass);
  const axis = div().h_flex().ml(22)
    .children(Array.from({ length: SIZE }, (_, column) => div().w(size).text_center().text_xs()
      .text_color(material.hint).child(String.fromCharCode(65 + column))));
  const rows = div().v_flex().w(22)
    .children(Array.from({ length: SIZE }, (_, row) => div().w(22).h(size).h_flex()
      .items_center().justify_center().text_xs().text_color(material.hint).child(String(row + 1))));
  const grid = div().id("reversi-board").v_flex().w(size * SIZE).h(size * SIZE)
    .border_1().border_color(material.line).flex_shrink(0)
    .children(view.game.board.map((_, row) => div().h_flex().h(size)
      .children(Array.from({ length: SIZE }, (_, column) => cell(view, row, column, size,
        offered.has(row * SIZE + column), last)))));
  return div().id("reversi-board-wrap").v_flex().flex_shrink(0).items_center().gap_1()
    .child(axis)
    .child(div().h_flex().child(rows).child(grid));
}
