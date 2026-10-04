import { Header, Image, theme } from "sailry";
import { Modal } from "sailry/ui";
import { div } from "gpui-kit";
import { Spinner } from "gpui-component";
import { board } from "./board.js";
import { stage, portrait } from "./layout.js";
import { action, piece, status } from "./controls.js";
import { history } from "./history.js";
import { result } from "./result.js";

function lobby(view, header) {
  const { text } = view, colors = theme().colors;
  const status = view.loading ? "loading" : view.available ? "ready" : view.error || "configure";
  const card = div().id("gomoku-lobby").v_flex().items_center().justify_between().gap_5()
    .w_full().max_w(420).min_h(320).p_6()
    .child(div().h_flex().items_center().gap_3().p_4()
      .child(piece(1, 32)).child(piece(2, 32)))
    .child(div().v_flex().items_center().gap_1().text_center()
      .child(div().text_3xl().font_bold().text_color(colors.foreground).child(text.title))
      .child(div().text_sm().text_color(colors.muted_foreground).child(text.subtitle)))
    .child(div().id(`gomoku-status-${status}`).text_sm().text_center()
      .text_color(colors.muted_foreground)
      .child(view.loading || status === "loadFailed" ? "" : text[status]));
  if (view.loading) card.child(new Spinner());
  else if (view.available) card.child(div().h_flex().flex_wrap().justify_center().gap_3()
    .child(action("gomoku-start-black", text.startBlack, cx => view.start(cx, 1)).primary())
    .child(action("gomoku-start-white", text.startWhite, cx => view.start(cx, 2)).outline()));
  return div().size_full().v_flex().child(header)
    .child(div().flex_1().w_full().h_flex().items_center().justify_center().p_5().child(card));
}

function seat(view, player, first) {
  const { game, text } = view, colors = theme().colors;
  const active = game.phase === "play" && game.turn === player;

  return div().id(`gomoku-seat-${player}`).relative().h(88).flex_1().min_w(0).px_4().py_3()
    .border_r(first ? 1 : 0).border_color(colors.border)
    .child(div().h_flex().h_full().items_center().gap_3()
      .child(div().w(40).h(40).flex_shrink(0)
        .child(Image.new(`gomoku-avatar-${player}`, { path: portrait(view, player) })))
      .child(div().v_flex().flex_1().min_w(0).gap_1()
        .child(div().text_sm().font_semibold().truncate().child(player === game.human ? text.you : view.name))
        .child(div().h_flex().items_center().gap_2().text_xs().text_color(colors.muted_foreground)
          .child(piece(player, 14)).child(player === 1 ? text.black : text.white)
          .child(player === 1 ? text.first : text.second))))
    .children(active ? [div().id(`gomoku-active-${player}`).absolute().left(16).right(16).bottom(0).h(2).bg(colors.ring)] : []);
}

function table(view, header) {
  const colors = theme().colors;
  const height = Math.max(448, Math.min(788, window.viewport_size().height - 196));
  const content = div().id("gomoku-content").w_full().max_w(1240).min_w(720).v_flex()
    .child(div().id("gomoku-player-strip").h_flex().h(89).flex_shrink(0).border_b_1().border_color(colors.border)
      .child(seat(view, view.game.human, true)).child(seat(view, 3 - view.game.human, false)))
    .child(div().h_flex().h(height).items_stretch()
      .child(div().id("gomoku-board-region").v_flex().flex_1().min_w(0).items_center().justify_center().gap_4().p_4()
        .child(board(view)).child(status(view)))
      .child(history(view)));
  const showResult = view.game.phase === "over" && view.motion.result?.visible && !view.resultDismissed;
  return div().id("gomoku-table").size_full().v_flex().min_w(0).min_h(0).overflow_hidden()
    .text_color(colors.foreground).child(header).child(stage(content, 720))
    .child(Modal.new(`gomoku-result-${view.resultId}`, { open: !!showResult })
      .children(showResult ? [result(view)] : []));
}

export function render(view) {
  const { text } = view;
  const header = Header.new("gomoku-header", { content: JSON.stringify({ items: [
    { label: text.wins, value: String(view.wins), tone: "success" },
    { label: text.losses, value: String(view.losses), tone: "danger" },
    { label: text.draws, value: String(view.draws) },
  ], actions: view.game ? [{ id: "gomoku-header-new-game", label: text.newGame,
    icon: "icons/rotate-cw.svg" }] : [] }) });
  return view.game ? table(view, header) : lobby(view, header);
}
