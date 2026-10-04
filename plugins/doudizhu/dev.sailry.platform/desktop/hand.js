import { div } from "gpui-kit";
import { card } from "./table.js";
import { tween } from "./effects.js";
import { sweep } from "./selection.js";

export function hand(view, cx, compact) {
  const values = view.game.hands[0];
  const enabled = ["bid", "play"].includes(view.game.phase);
  const padding = compact ? 32 : 48;
  const spacing = compact ? 48 : 68;
  const begin = (index, cx) => {
    cx.stop_propagation();
    view.skipClick = false;
    if (enabled) view.drag = { start: index, end: index, selected: [...view.selected] };
  };
  const extend = (index, cx) => {
    if (!enabled || !view.drag || index === view.drag.end) return;
    view.drag.end = index;
    view.skipClick = true;
    view.hintPass = false;
    view.selected = sweep(values, view.drag.selected, view.drag.start, index);
    cx.notify();
  };
  const row = div().id("ddz-hand").h_flex().flex_shrink(0).justify_center().w_full()
    .max_w(values.length * spacing + padding).mx_auto().pr(padding).min_h(compact ? 72 : 104)
    .on_mouse_up("left", () => { view.drag = null; })
    .on_hover(hovered => { if (!hovered) view.drag = null; })
    .on_key_down(() => { view.skipClick = false; view.drag = null; });
  return row.children(values.map((value, index) => {
    const selected = view.selected.includes(value);
    return div().id(`ddz-${selected ? "selected" : "slot"}-${value}`).relative().flex_grow(1)
      .min_w(0).w(compact ? 22 : 40).max_w(spacing).h(compact ? 72 : 104)
      .child(tween(div().id(`ddz-hand-motion-${value}`).absolute().left_0())
        .on_mouse_down("left", (_event, cx) => begin(index, cx))
        .on_mouse_move((_event, cx) => {
          if (!view.drag) return;
          cx.stop_propagation();
          extend(index, cx);
        })
        .opacity(view.motion?.dealing != null && view.motion.dealing < 3 ? 0 : 1)
        .top(view.motion?.dealing != null && view.motion.dealing < 3 ? 30 : selected ? 0 : compact ? 8 : 12)
        .child(card(value, cx, view.text, selected, enabled ? (_event, cx) => {
          cx.stop_propagation();
          view.drag = null;
          if (view.skipClick) return;
          view.hintPass = false;
          view.selected = selected ? view.selected.filter(card => card !== value) : [...view.selected, value];
          cx.notify();
        } : null, compact, compact)));
  }));
}
