import { div } from "gpui-kit";
import { Button, ShimmerText } from "gpui-component";
import { Image, theme } from "sailry";
import { BIG_BLIND, legal } from "./game.js";
import { portrait } from "./layout.js";
import { retryable } from "./model.js";

export function button(id, label, action, kind = "outline") {
  const control = new Button(id).size("small").h(34).rounded_lg().min_w(76)
    .on_click((_event, cx) => action(cx)).label(label);
  return kind === "primary" ? control.primary() : control.outline();
}

export function raiseTargets(game) {
  const options = legal(game, 0);
  if (!options?.canRaise) return [];
  const base = game.currentBet || BIG_BLIND;
  return [2, 3, 5, 10].map(factor => ({ factor, to: base * factor,
    enabled: base * factor >= options.minTo && base * factor <= options.maxTo }));
}

export function raiseDialog(view) {
  const { game, text } = view, options = legal(game, 0), colors = theme().colors;
  const targets = raiseTargets(game);
  const choose = (to, cx) => { view.raiseTo = to; cx.notify(); };
  const grid = div().id("poker-raise-grid").v_flex().w_full().gap_2()
    .children([0, 2].map(start => div().h_flex().w_full().gap_2()
      .children(targets.slice(start, start + 2).map(option =>
        button(`poker-raise-${option.factor}x`, `${option.factor}×   ${option.to}`,
          cx => choose(option.to, cx), view.raiseTo === option.to ? "primary" : "outline")
          .flex_1().h(52).disabled(!option.enabled)))));
  return div().id("poker-raise-dialog").w(320).v_flex().gap_4()
    .child(div().v_flex().gap_1()
      .child(div().text_sm().text_color(colors.muted_foreground).child(text.raiseTo))
      .child(div().id("poker-raise-amount").text_3xl().font_bold().child(String(view.raiseTo)))
      .children(options.canShortRaise ? [] : [div().text_xs().text_color(colors.muted_foreground)
        .child(game.currentBet ? text.raiseBase(game.currentBet) : text.blindBase(BIG_BLIND))]))
    .children(options.canShortRaise ? [] : [grid])
    .child(div().h_flex().w_full().gap_2()
      .children(options.minTo < options.maxTo ? [button("poker-min", `${text.min} ${options.minTo}`,
        cx => choose(options.minTo, cx)).flex_1()] : [])
      .child(button("poker-all-in", `${text.allIn} ${options.maxTo}`, cx => choose(options.maxTo, cx)).flex_1()))
    .child(div().h_flex().justify_end().gap_2().pt_4().border_t_1().border_color(colors.border)
      .child(button("poker-cancel-raise", text.cancel, cx => { view.raiseOpen = false; cx.notify(); }))
      .child(button("poker-raise", text.confirm, cx => view.raise(cx), "primary")));
}

function controls(view, options) {
  const { text } = view;
  const actions = div().id("poker-controls").h_flex().items_center().justify_center().flex_wrap().gap_2();
  if (options.toCall) actions.child(button("poker-fold", text.fold, cx => view.act({ kind: "fold" }, cx)));
  actions.child(options.toCall
    ? div().id("poker-action-call").child(button("poker-call", `${text.call} ${options.callAmount}`,
      cx => view.act({ kind: "call" }, cx), "primary"))
    : div().id("poker-action-check").child(button("poker-check", text.check,
      cx => view.act({ kind: "check" }, cx), "primary")));
  if (options.canRaise) actions.child(button("poker-open-raise", options.canShortRaise ? text.allIn : text.raise, cx => {
    view.setAmount();
    view.raiseId++;
    view.raiseOpen = true;
    cx.notify();
  }));
  return actions;
}

export function actionRegion(view) {
  const { game, text } = view, colors = theme().colors;
  const region = div().id("poker-action-region").v_flex().min_h(90).w_full()
    .items_center().justify_center().gap_3();
  if (game.phase === "over") {
    const matchOver = game.settlement.matchOver;
    return region.child(button(matchOver ? "poker-table-new-match" : "poker-table-next-hand",
      matchOver ? text.newMatch : text.next, cx => matchOver ? view.reset(cx) : view.next(cx), "primary"));
  }
  if (retryable.includes(view.error)) region.child(button("poker-retry", text.retry, cx => view.advance(cx)));
  const options = !view.busy && legal(game, 0);
  if (options) return region.child(controls(view, options));
  region.child(div().id(`poker-status-${view.busy ? "thinking" : "waiting"}`)
    .h_flex().flex_wrap().min_w(0).items_center().justify_center().gap_2().text_sm().text_color(colors.muted_foreground)
    .child(div().w(24).h(24).flex_shrink(0).accessibility_label(view.names[game.turn])
      .child(Image.new("poker-status-avatar", { path: portrait(game.turn) })))
    .child(view.busy ? new ShimmerText(text.pondering).id("poker-thinking").text_sm() : div().child(text.waiting))
    .children(view.busy ? [div().id("poker-elapsed").text_xs().child(`${view.elapsed} ${text.seconds}`)] : []));
  return region;
}
