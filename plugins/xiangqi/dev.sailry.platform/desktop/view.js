import { Header, theme } from "sailry";
import { div } from "gpui-kit";
import { Button, ShimmerText, Spinner, Tag } from "gpui-component";
import { board } from "./board.js";
import { coordinate } from "./game.js";
import { retryable } from "./model.js";

function action(id, label, onClick) {
  return new Button(id).size("small").h(36).rounded(9999).min_w(96)
    .label(label).on_click((_event, cx) => onClick(cx));
}

function emblem(text, side, diameter = 36) {
  return div().w(diameter).h(diameter).flex_shrink(0).h_flex().items_center().justify_center()
    .rounded(9999).border_2().border_color(side === "red" ? "#9e4436" : "#45413b")
    .bg("#f3dfb6").font_bold().text_color(side === "red" ? "#a3332d" : "#303338")
    .child(text.glyphs[side === "red" ? "K" : "k"]);
}

function lobby(view, header) {
  const { text } = view, colors = theme().colors;
  const key = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const card = div().id("xiangqi-lobby").v_flex().items_center().justify_between().gap_4()
    .w_full().max_w(440).min_h(340).p_6()
    .child(div().h_flex().items_center().gap_3().p_4().rounded_lg().bg(colors.muted)
      .child(emblem(text, "black", 42)).child(emblem(text, "red", 42)))
    .child(div().v_flex().items_center().gap_1().text_center()
      .child(div().text_3xl().font_bold().text_color(colors.foreground).child(text.title))
      .child(div().text_sm().text_color(colors.muted_foreground).child(text.subtitle)))
    .child(div().id(`xiangqi-status-${key}`).text_sm().text_center()
      .text_color(colors.muted_foreground)
      .child(view.loading || key === "loadFailed" ? "" : text[key]));
  if (view.loading) card.child(new Spinner());
  else if (view.available) card.child(div().h_flex().flex_wrap().justify_center().gap_3()
    .child(div().id("xiangqi-choice-red")
      .child(action("xiangqi-start-red", text.startRed, cx => view.start(cx, "red")).primary()))
    .child(div().id("xiangqi-choice-black")
      .child(action("xiangqi-start-black", text.startBlack, cx => view.start(cx, "black")).outline())));
  card.child(div().text_xs().text_center().text_color(colors.muted_foreground)
    .child(text.practiceRule));
  return div().size_full().v_flex().child(header)
    .child(div().flex_1().w_full().h_flex().items_center().justify_center().p_5().child(card));
}

function seat(view, side) {
  const { game, text } = view, colors = theme().colors;
  const active = game.phase === "play" && game.turn === side;
  const name = side === game.human ? text.you : view.name;
  return div().id(`xiangqi-seat-${side}`).h_flex().flex_1().min_w(0).items_center().gap_2()
    .p_2().rounded_lg().border_1().border_color(active ? colors.ring : colors.border)
    .bg(active ? colors.accent : colors.muted)
    .child(emblem(text, side, 32))
    .child(div().v_flex().min_w(0)
      .child(div().truncate().text_sm().font_medium().text_color(colors.foreground).child(name))
      .child(div().text_xs().text_color(colors.muted_foreground)
        .child(`${text[side]} · ${side === "red" ? text.first : text.second}`)));
}

function status(view) {
  const { game, text } = view, colors = theme().colors;
  const key = game.phase === "over" ? game.winner === null ? "draw"
    : game.winner === game.human ? "win" : "lose"
    : view.busy ? "thinking" : game.turn === game.human ? "yourTurn" : "paused";
  const line = div().id(`xiangqi-status-${key}`).h_flex().items_center().gap_2().min_h(28)
    .text_sm().font_medium().text_color(colors.foreground);
  if (key === "thinking") line.child(new ShimmerText(text.thinking).id("xiangqi-thinking").text_sm())
    .child(div().text_color(colors.muted_foreground).child(`· ${view.elapsed} ${text.seconds}`));
  else line.child(text[key]);
  if (game.phase === "play" && game.check)
    line.child(new Tag().variant("warning").size("small").child(text.check));
  if (retryable.includes(view.error)) line.child(action("xiangqi-retry", text.retry,
    cx => view.advance(cx)).outline().h(28));
  return line;
}

function history(view) {
  const { game, text } = view, colors = theme().colors;
  const moves = game.moves;
  return div().id("xiangqi-history").v_flex().w(240).min_h(350).p_4().gap_3()
    .rounded_2xl().border_1().border_color(colors.border).bg(colors.background)
    .child(div().h_flex().items_center().justify_between().border_b_1().border_color(colors.border).pb_3()
      .child(div().text_base().font_semibold().text_color(colors.foreground).child(text.moves))
      .child(new Tag().variant("secondary").size("small").child(String(moves.length))))
    .child(div().v_flex().max_h(360).overflow_y_scroll().gap_1()
      .children(moves.length ? moves.map((move, index) => div().id(`xiangqi-move-${index + 1}`)
        .h_flex().items_center().gap_2().py_2().px_2().rounded_md()
        .bg(index === moves.length - 1 ? colors.muted : colors.background)
        .child(div().w(25).text_xs().text_color(colors.muted_foreground).child(String(index + 1)))
        .child(div().text_sm().text_color(colors.foreground)
          .child(text.playerMove(move.side === game.human ? text.you : view.name,
            coordinate(move.fromRow, move.fromColumn), coordinate(move.toRow, move.toColumn),
            text.glyphs[move.piece]))))
        : [div().text_sm().text_color(colors.muted_foreground).child(text.noMoves)]))
    .child(div().text_xs().text_color(colors.muted_foreground).child(text.practiceRule));
}

function table(view, header) {
  const { game, text } = view, colors = theme().colors;
  const last = game.moves.at(-1);
  const ended = game.phase === "over";
  const outcome = game.winner === null ? "draw" : game.winner === game.human ? "win" : "lose";
  const card = div().id("xiangqi-table-card").v_flex().items_center().gap_3().w_full().max_w(550)
    .p_4().rounded_2xl().border_1().border_color(colors.border).bg(colors.background)
    .child(div().w_full().h_flex().items_center().justify_between().gap_2()
      .child(status(view))
      .child(new Tag().variant("secondary").outline().size("small").child(text.subtitle)))
    .child(div().w_full().h_flex().items_center().gap_3()
      .child(seat(view, "black")).child(seat(view, "red")))
    .child(div().w_full().overflow_x_scroll().child(board(view)))
    .child(div().w_full().h_flex().items_center().justify_between().gap_3()
      .child(div().text_sm().text_color(colors.muted_foreground)
        .child(last ? `${text.lastMove} · ${coordinate(last.fromRow, last.fromColumn)}–${coordinate(last.toRow, last.toColumn)}`
          : text.noMoves))
      .child(div().id("xiangqi-new-game-action")
        .child(action("xiangqi-new-game", ended ? text.again : text.newGame,
          cx => view.start(cx, game.human)).outline())));
  if (ended) card.child(div().id("xiangqi-result").relative().h_flex().items_center().gap_2()
    .top(view.motion?.resultVisible === false ? 8 : 0)
    .opacity(view.motion?.resultVisible === false ? 0 : 1)
    .transition("top", { duration: 240, easing: "ease-out" })
    .transition("opacity", { duration: 240, easing: "ease-out" })
    .child(new Tag().variant(outcome === "win" ? "success" : outcome === "lose" ? "danger" : "secondary")
      .child(text[outcome]))
    .child(div().text_sm().text_color(colors.muted_foreground).child(text[game.reason])));
  return div().id("xiangqi-table").size_full().v_flex().min_w(0).min_h(0).overflow_y_scroll()
    .child(header)
    .child(div().w_full().h_flex().flex_wrap().items_start().justify_center().gap_4().p_4()
      .child(card).child(history(view)));
}

export function render(view) {
  const { text } = view;
  const header = Header.new("xiangqi-header", { content: JSON.stringify({ items: [
    { label: text.wins, value: String(view.wins), tone: "success" },
    { label: text.losses, value: String(view.losses), tone: "danger" },
    { label: text.draws, value: String(view.draws) },
  ], actions: view.game ? [{ id: "xiangqi-header-new-game", label: text.newGame,
    icon: "icons/rotate-cw.svg" }] : [] }) });
  return view.game ? table(view, header) : lobby(view, header);
}
