import {View, div} from "gpui-kit";
import {nextChange, readSettings} from "sailry/sdk";
import {IconButton, nextControlEvent} from "sailry/ui";

export default class Cached extends View {
  init(_props, cx) {
    this.reads = 0;
    this.draft = 0;
    cx.spawn(async cx => {
      let cursor = "";
      try { while (true) {
        const change = await nextChange(cursor); cursor = change.cursor;
        if (change.connected) { await readSettings(); this.reads++; cx.notify(); }
      } } catch (_) { /* The fixture releases its subscriptions with the view. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        if ((await nextControlEvent()).id === "entry-edit") { this.draft++; cx.notify(); }
      } } catch (_) { /* The fixture releases its controls with the view. */ }
    });
  }
  render() {
    return div().v_flex().size_full()
      .child(div().id(`entry-reads-${this.reads}`).child(String(this.reads)))
      .child(div().id(`entry-draft-${this.draft}`).child(String(this.draft)))
      .child(IconButton.new("entry-edit", {icon:"plus", label:"Edit"}));
  }
}
