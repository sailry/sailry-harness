import { div } from "gpui-kit";
import { Button } from "gpui-base";
import { Image } from "sailry";
import { legalMoves, coordinate, sideOf, ROWS, COLUMNS } from "./game.js";

// GPUI Kit has no Xiangqi board control. This original vector draws its grid;
// Kit buttons handle intersection input and the surrounding UI uses Kit controls.
const material = { red: "#a8362f", black: "#303338", face: "#f7e8bd",
  edge: "#9c7544", legal: "#287c55", last: "#cf8b32" };

function disk(view, token, id, diameter, selected = false, recent = false) {
  const side = sideOf(token);
  return div().id(id).w(diameter).h(diameter).h_flex().items_center().justify_center()
    .rounded(9999).border_2().border_color(selected ? material.legal
      : recent ? material.last : material.edge)
    .bg(material.face).shadow_md().font_bold().text_2xl()
    .text_color(side === "red" ? material.red : material.black)
    .child(view.text.glyphs[token]);
}

function intersection(view, row, column, cell, allowed, legal, recent, active) {
  const token = view.game.board[row][column];
  const selected = view.selected?.row === row && view.selected?.column === column;
  const target = legal.some(move => move.toRow === row && move.toColumn === column);
  const detail = token === "." ? view.text.empty
    : `${sideOf(token) === "red" ? view.text.red : view.text.black} ${view.text.pieceNames[token.toLowerCase()]}`;
  const content = div().relative().size_full().h_flex().items_center().justify_center();
  if (selected) content.child(div().id(`xiangqi-selected-${row}-${column}`)
    .absolute().left(1).top(1).w(cell - 2).h(cell - 2).rounded(9999)
    .border_2().border_color(material.legal));
  if (target) content.child(div().id(`xiangqi-legal-${row}-${column}`)
    .absolute().left(token === "." ? cell / 2 - 5 : 1)
    .top(token === "." ? cell / 2 - 5 : 1)
    .w(token === "." ? 10 : cell - 2).h(token === "." ? 10 : cell - 2)
    .rounded(9999).bg(token === "." ? material.legal : "#00000000")
    .border_color(material.legal).border_1());
  if (token !== ".") {
    const hidden = active && active.stage < 3
      && active.move.toRow === row && active.move.toColumn === column;
    content.child(disk(view, token, `xiangqi-piece-${row}-${column}`, cell - 7, selected, recent)
      .relative().opacity(hidden ? 0 : 1)
      .transition("opacity", { duration: 130, easing: "ease-out" }));
  }
  return Button.new(`xiangqi-cell-${row}-${column}`).w(cell).h(cell).p_0().bg("#00000000")
    .accessibility_label(view.text.cell(coordinate(row, column), detail))
    .disabled(!allowed)
    .on_click((_event, cx) => { cx.stop_propagation(); view.act(row, column, cx); })
    .child(content);
}

function point(row, column, flipped, cell) {
  return { left: (flipped ? COLUMNS - 1 - column : column) * cell,
    top: (flipped ? ROWS - 1 - row : row) * cell };
}

function pulse(id, location, cell, stage, color) {
  const expanded = stage >= 3, diameter = expanded ? cell + 16 : cell - 8;
  return div().id(id).absolute()
    .left(location.left + (cell - diameter) / 2)
    .top(location.top + (cell - diameter) / 2)
    .w(diameter).h(diameter).rounded(9999).border_2().border_color(color)
    .opacity(expanded ? 0 : 0.9)
    .transition("left", { duration: 300, easing: "ease-out" })
    .transition("top", { duration: 300, easing: "ease-out" })
    .transition("width", { duration: 300, easing: "ease-out" })
    .transition("height", { duration: 300, easing: "ease-out" })
    .transition("opacity", { duration: 300, easing: "ease-out" });
}

function effects(view, active, cell, flipped) {
  if (!active) return [];
  const { id, move, stage } = active;
  const from = point(move.fromRow, move.fromColumn, flipped, cell);
  const to = point(move.toRow, move.toColumn, flipped, cell);
  const diameter = cell - 7, inset = (cell - diameter) / 2;
  const moving = stage >= 1 ? to : from;
  const layers = [disk(view, move.piece, `xiangqi-moving-piece-${id}`, diameter)
    .absolute().left(moving.left + inset).top(moving.top + inset)
    .transition("left", { duration: 290, easing: "ease-out" })
    .transition("top", { duration: 290, easing: "ease-out" })
    .opacity(stage >= 3 ? 0 : 1)
    .transition("opacity", { duration: 100, easing: "ease-out" })];
  if (move.captured !== ".") {
    layers.push(disk(view, move.captured, `xiangqi-captured-piece-${id}`, diameter)
      .absolute().left(to.left + inset).top(to.top + inset - (stage ? 9 : 0))
      .opacity(stage ? 0 : 1)
      .transition("top", { duration: 180, easing: "ease-out" })
      .transition("opacity", { duration: 180, easing: "ease-out" }));
    if (stage >= 2) layers.push(pulse(`xiangqi-capture-pulse-${id}`, to, cell, stage, material.last));
  }
  if (active.check && stage >= 2) {
    const general = view.game.turn === "red" ? "K" : "k";
    for (let row = 0; row < ROWS; row++) for (let column = 0; column < COLUMNS; column++)
      if (view.game.board[row][column] === general)
        layers.push(pulse(`xiangqi-check-pulse-${id}`, point(row, column, flipped, cell),
          cell, stage, material.red));
  }
  return layers;
}

export function board(view) {
  const size = window.viewport_size();
  const cell = size.height < 760 || size.width < 900 ? 39 : 46;
  const flipped = view.game.human === "black";
  const active = view.motion?.active;
  const rows = Array.from({ length: ROWS }, (_, index) => flipped ? ROWS - 1 - index : index);
  const columns = Array.from({ length: COLUMNS }, (_, index) => flipped ? COLUMNS - 1 - index : index);
  const legal = view.selected && view.game.phase === "play" && view.game.turn === view.game.human
    ? legalMoves(view.game.board, view.game.human, view.selected.row, view.selected.column) : [];
  const allowed = view.game.phase === "play" && view.game.turn === view.game.human
    && !view.busy && (!active || active.stage >= 3);
  const last = view.game.moves.at(-1);
  const grid = div().id("xiangqi-board").relative().w(cell * COLUMNS).h(cell * ROWS)
    .flex_shrink(0).rounded_lg().overflow_hidden().shadow_sm()
    .child(div().absolute().left(0).top(0).size_full()
      .child(Image.new("xiangqi-grid", { path: "dev.sailry.platform/desktop/board.svg" })))
    .child(div().absolute().left(0).top(0).v_flex()
      .children(rows.map(row => div().h_flex().h(cell)
        .children(columns.map(column => intersection(view, row, column, cell, allowed, legal,
          last?.toRow === row && last?.toColumn === column, active))))))
    .children(effects(view, active, cell, flipped));
  const axis = div().h_flex().ml(20).w(cell * COLUMNS)
    .children(columns.map(column => div().w(cell).text_center().text_xs()
      .text_color(material.edge).child(String.fromCharCode(65 + column))));
  const ranks = div().v_flex().w(20)
    .children(rows.map(row => div().w(20).h(cell).h_flex().items_center().justify_center()
      .text_xs().text_color(material.edge).child(String(ROWS - row))));
  return div().id("xiangqi-board-wrap").v_flex().items_center().gap_1().p_2()
    .rounded_lg().bg("#e2c697").border_1().border_color(material.edge)
    .child(div().h_flex().child(ranks).child(grid)).child(axis);
}
