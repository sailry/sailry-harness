import { Header, theme } from "sailry";
import { Modal } from "sailry/ui";
import { details, eventDetails } from "./details.js";
import { result } from "./result.js";
import { div } from "gpui-kit";
import { board } from "./board.js";
import { stage } from "./layout.js";
import { MAP_SIZE } from "./map.js";
import { lobby } from "./lobby.js";
import { seat, portfolio, assets, auction, activity, AUCTION_HEIGHT, ASSETS_HEIGHT } from "./portfolio.js";
import { button } from "./controls.js";

const LEFT_WIDTH = 216;
const RIGHT_WIDTH = 232;
const BOARD_PADDING = 16;
const MAX_WIDTH = 1240;
const MIN_CELL = 88;
const PLAYER_HEIGHT = 89;

function dimensions() {
  const cell = Math.max(MIN_CELL, Math.min(108, Math.floor((window.viewport_size().height - 228) / MAP_SIZE)));
  return { cell, width: LEFT_WIDTH + RIGHT_WIDTH + BOARD_PADDING * 2 + 2 + cell * MAP_SIZE };
}

function table(view, header) {
  const { game, text } = view, colors = theme().colors;
  const { cell, width } = dimensions();
  const boardSize = cell * MAP_SIZE;
  const panelHeight = boardSize + BOARD_PADDING * 2;
  const content = div().id(`city-state-${game.phase}-${game.turn}-${game.round}`)
    .relative().w_full().min_w(width).max_w(MAX_WIDTH).flex_shrink(0).v_flex()
    .child(div().id("city-player-strip").relative().h_flex().w_full().h(PLAYER_HEIGHT).flex_shrink(0)
      .border_b_1().border_color(colors.border)
      .children(game.players.map((player, index) => player.enabled ? seat(view, index) : null).filter(Boolean)))
    .child(div().id("city-regions").relative().h_flex().w_full().h(panelHeight).flex_shrink(0)
      .child(div().id("city-left-region").w(LEFT_WIDTH).h_full().flex_shrink(0).p_4()
        .border_r_1().border_color(colors.border).child(portfolio(view, panelHeight - 32)))
      .child(div().id("city-board-region").flex_1().min_w(boardSize + BOARD_PADDING * 2).h_full()
        .h_flex().items_center().justify_center().p_4().child(board(view, cell)))
      .child(div().id("city-right-region").w(RIGHT_WIDTH).h_full().flex_shrink(0).v_flex()
        .border_l_1().border_color(colors.border)
        .children(game.auction ? [auction(view)] : [])
        .child(assets(view)).child(activity(view, panelHeight - ASSETS_HEIGHT
          - (game.auction ? AUCTION_HEIGHT : 0)))))
    .children(view.saveError === "saveConflict" ? [div().p_4()
      .child(button("city-refresh-save", text.refreshSave, cx => view.refreshSave(cx), view.saving))] : []);
  return div().id(`city-table-save-${view.saving ? "pending" : view.saved?.revision || "0"}`)
    .size_full().v_flex().min_w(0).min_h(0).overflow_hidden().text_color(colors.foreground)
    .child(header).child(stage(content, width));
}

export function render(view) {
  const { text } = view;
  const header = Header.new("city-header", { content: JSON.stringify({
    min_content_width: view.game ? dimensions().width + 32 : 0,
    min_content_height: view.game ? MIN_CELL * MAP_SIZE + PLAYER_HEIGHT + BOARD_PADDING * 2 + 32 : 0,
    items: view.game ? [{ label: text.round(Math.min(view.game.round, view.game.limit), view.game.limit), value: text.phase[view.game.phase] }] : [],
    actions: view.game ? [{ id: "city-header-save", label: view.saving ? text.saving : text.save, icon: "icons/save.svg", disabled: !!(view.saving || view.busy || view.animating || view.pending || (view.game.phase !== "over" && view.game.turn !== 0)) }, { id: "city-header-new-game", label: text.again,
      icon: "icons/rotate-cw.svg", disabled: !!(view.saving || view.savePending) }] : [],
  }) });
  return (view.game ? table(view, header) : lobby(view, header))
    .child(Modal.new(`city-dialog-${view.dialogId}`, { open: view.dialogOpen })
      .children(view.dialogOpen && view.game ? [view.dialogKind === "result" ? result(view)
        : view.dialogKind === "event" ? eventDetails(view) : details(view)] : []));
}
