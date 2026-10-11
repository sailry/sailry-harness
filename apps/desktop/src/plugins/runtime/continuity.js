import {View, div} from "gpui-kit";
import {Button} from "gpui-component";
import {prepare, execute, forget} from "sailry";
import {readSettings, nextChange} from "sailry/sdk";
import {createText, readText, TextField, nextTextEvent} from "sailry/forms";
import {ScrollRegion} from "sailry/ui";

export default class Settings extends View {
  init(_props, cx) {
    this.draft = createText("Draft", {label: "Draft"});
    this.value = null;
    this.package = null;
    this.saving = false;
    this.failure = "";
    cx.spawn(async cx => {
      try {
        await this.load();
        cx.notify();
        let cursor = "";
        while (true) {
          const change = await nextChange(cursor);
          cursor = change.cursor;
          if (change.connected && !this.saving) {
            await this.load();
            cx.notify();
          }
        }
      } catch (error) {
        this.failure = String(error);
        cx.notify();
      }
    });
    cx.spawn(async cx => {
      while (true) {
        const event = await nextTextEvent();
        if (event.id !== this.draft) continue;
        if (event.kind === "enter" && !this.saving) {
          this.saving = true;
          try {
            const id = prepare(JSON.stringify({kind: "save_plugin_settings", data: {
              package: this.package,
              values: {include_untracked: !this.value}, secrets: {}
            }}));
            const result = JSON.parse(await execute(id));
            if (!result.Ok || result.Ok.kind !== "plugin_settings") {
              throw new Error(JSON.stringify(result));
            }
            this.package = result.Ok.data.package;
            this.value = result.Ok.data.values.include_untracked;
            forget(id);
          } catch (error) {
            this.failure = String(error);
          } finally {
            this.saving = false;
          }
        }
        cx.notify();
      }
    });
  }

  async load() {
    const settings = await readSettings();
    this.package = settings.package;
    this.value = settings.values.include_untracked;
  }

  render() {
    return div().size_full().v_flex().p_4().gap_4()
      .child(TextField.new(this.draft))
      .child(div().id(`continuity-value-${this.value}`).child(String(this.value)))
      .child(div().id("continuity-draft").child(readText(this.draft)))
      .child(div().id("continuity-failure").child(this.failure))
      .child(ScrollRegion.new("continuity-scroll", {max_height: 240})
        .children(Array.from({length: ROWS}, (_, index) => div().h(48).flex_shrink_0()
          .child(new Button(`continuity-row-${index}`).label(`Row ${index}`).w_full()))));
  }
}
