import { View, div } from "gpui-kit";
import { context } from "sailry";
import { getValue, listKeys, setValue, completeRequest, forgetRequest,
  publishContributions, nextContributionEvent } from "sailry/sdk";
import { messages } from "./locales.js";

export default class Notes extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.preferences = null;
    this.keys = [];
    this.pending = null;
    this.query = "";
    cx.spawn(async () => {
      try {
        try { await this.refresh(); } catch (_) { /* Refresh remains available. */ }
        this.publish();
        while (true) {
          const event = await nextContributionEvent();
          try {
            if (event.kind === "search") {
              this.query = event.value;
              await this.refresh();
            } else if (event.handler === "refresh") {
              await this.refresh();
            } else {
              const values = { focus: false, level: "normal", selected: null, ...this.preferences.value };
              if (event.handler === "focus") values.focus = event.value;
              if (event.handler === "level") values.level = event.value;
              if (event.handler === "note") values.selected = event.value;
              if (event.handler === "actions") values.focus = event.value === "enable";
              this.pending = setValue("preferences", values, this.preferences.revision);
              await this.refresh();
            }
          } catch (_) { /* An uncertain request keeps its original ID for Refresh. */ }
          this.publish(event);
        }
      } catch (_) { /* The captured host closes when the contribution instance is released. */ }
    });
  }

  async refresh() {
    if (this.pending) {
      const result = await completeRequest(this.pending);
      const request = this.pending;
      this.pending = null;
      forgetRequest(request);
      if (result.Err) throw new Error(result.Err.code);
    }
    this.preferences = await getValue("preferences");
    let after = null;
    const keys = [];
    do {
      const page = await listKeys("", after, 100);
      keys.push(...page.keys.filter(key => key !== "preferences"));
      after = page.after;
    } while (after);
    this.keys = keys;
  }

  publish(event) {
    const values = this.preferences?.value || {};
    const enabled = !!this.preferences && !this.pending;
    const response = id => event?.id === id ? { reply_to: event.sequence } : {};
    publishContributions([
      { id: "focus", value: !!values.focus, enabled, ...response("focus") },
      { id: "level", value: values.level || "normal", enabled, ...response("level") },
      { id: "refresh", enabled: true, ...response("refresh") },
      { id: "actions", enabled, ...response("actions") },
      { id: "note", value: values.selected || null, enabled,
        choices: this.keys.filter(key => key.toLowerCase().includes(this.query.toLowerCase()))
          .map(key => ({ id: key, label: { label: key }, icon: "file-text" })), ...response("note") },
      { id: "notes", value: this.preferences ? this.keys.length : null,
        details: [{ label: { label: this.text.revision }, value: this.preferences?.revision || "0" },
          { label: { label: this.text.selected }, value: values.selected || this.text.none }] }
    ]);
  }

  render() { return div(); }
}
