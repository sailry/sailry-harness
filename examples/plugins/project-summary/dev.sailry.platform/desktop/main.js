import { View, div } from "gpui-kit";
import { Button, Checkbox } from "gpui-component";
import { context, prepare, execute, forget, next_change, theme } from "sailry";
import { inspectGit } from "sailry/sdk";
import {toast} from "sailry/ui";
import { messages } from "./locales.js";
import { path, report } from "./report.js";

export default class ProjectSummary extends View {
  init(_props, cx) {
    this.scope = JSON.parse(context());
    this.text = messages(this.scope.locale);
    this.include = null;
    this.status = null;
    this.revision = null;
    this.busy = false;
    this.pending = null;
    this.message = "loading";
    this.changed = false;
    this.connected = false;
    this.observed = false;
    this.changes = 0;
    this.canSave = false;
    cx.spawn(async (cx) => {
      let seen = "";
      try {
        while (true) {
          const next = await next_change(seen);
          const value = JSON.parse(next);
          const previous = seen ? JSON.parse(seen) : null;
          const initial = value.connected && !this.observed;
          if (this.observed && previous && value.revision !== previous.revision) {
            this.changes += 1;
            if (this.status) this.changed = true;
          }
          this.connected = value.connected;
          if (value.connected) this.observed = true;
          seen = next;
          // Establish observation before the first read so changes during that
          // read cannot disappear into the initial subscription snapshot.
          if (initial) this.refresh(cx);
          cx.notify();
        }
      } catch (_) {
        this.connected = false;
        if (!this.status) this.message = "failed";
        cx.notify();
      }
    });
  }

  async read(kind, data) {
    const id = prepare(JSON.stringify({ kind, data }));
    try { return JSON.parse(await execute(id)); }
    finally { forget(id); }
  }

  refresh(cx) {
    if (this.busy || this.pending) return;
    this.busy = true;
    this.canSave = false;
    this.message = "loading";
    const changes = this.changes;
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        if (this.include === null) {
          const settings = await this.read("read_plugin_settings", { package: this.scope.package });
          if (settings.Ok?.kind !== "plugin_settings" || typeof settings.Ok.data.values.include_untracked !== "boolean") {
            this.message = "settingsFailed";
            return;
          }
          this.include = settings.Ok.data.values.include_untracked;
        }
        const status = await inspectGit();
        const file = await this.read("read_file", { worktree: this.scope.worktree, path });
        if (file.Err?.code === "not_found") this.revision = null;
        else if (file.Ok?.kind === "file_content") {
          if (file.Ok.data.truncated || !file.Ok.data.revision) {
            this.message = "tooLarge";
            return;
          }
          this.revision = file.Ok.data.revision;
        } else throw new Error();
        this.status = status;
        this.changed = this.changes !== changes;
        this.canSave = true;
        this.message = "ready";
      } catch (_) {
        this.message = "failed";
      } finally {
        this.busy = false;
        this.notice();
        cx.notify();
      }
    });
  }

  save(cx) {
    if (this.busy || (!this.pending && !this.canSave)) return;
    this.busy = true;
    this.message = "saving";
    cx.notify();
    cx.spawn(async (cx) => {
      try {
        if (!this.pending) this.pending = prepare(JSON.stringify({
          kind: "write_file",
          data: { worktree: this.scope.worktree, path, expected_revision: this.revision,
            text: report(this.status, this.include, this.text) },
        }));
        const result = JSON.parse(await execute(this.pending));
        if (result.Ok?.kind === "file_written") {
          this.revision = result.Ok.data.revision;
          this.message = "saved";
          // Only Refresh establishes a new Git snapshot. A save receipt must
          // not clear file notifications received while that write was pending.
        } else if (["unavailable", "busy", "outcome_unknown", "internal", "cancelled"].includes(result.Err?.code)) {
          this.message = "unknown";
          return;
        } else {
          this.message = ["revision_conflict", "conflict"].includes(result.Err?.code) ? "conflict" : "saveFailed";
          this.canSave = false;
        }
        forget(this.pending);
        this.pending = null;
      } catch (_) {
        this.message = this.pending ? "unknown" : "saveFailed";
      } finally {
        this.busy = false;
        this.notice();
        cx.notify();
      }
    });
  }
  notice() {
    if(['failed','settingsFailed','tooLarge','saveFailed','conflict','unknown','saved'].includes(this.message))
      toast({id:'project-summary-result',kind:this.message==='saved'?'info':'error',message:this.text[this.message]});
  }

  render(cx) {
    const text = this.text;
    return div().v_flex().size_full().min_w_0().min_h_0().p_4().gap_3()
      .child(new Checkbox("summary-untracked").label(text.include).checked(this.include === true)
        .disabled(this.busy || !!this.pending || this.include === null)
        .on_change((checked, cx) => { this.include = checked; this.message = "ready"; cx.notify(); }))
      .child(div().h_flex().gap_2()
        .child(new Button("summary-refresh").label(text.refresh).disabled(this.busy || !!this.pending || !this.observed)
          .on_click((_event, cx) => this.refresh(cx)))
        .child(new Button("summary-save").primary().label(this.pending ? text.retry : text.save)
          .disabled(this.busy || (!this.canSave && !this.pending))
          .on_click((_event, cx) => this.save(cx))))
      .children(['loading','saving','ready'].includes(this.message)?[div().id(`summary-status-${this.message}`).text_sm().child(this.message==='ready'?'':text[this.message])]:[])
      .child(div().id(`summary-watch-${this.changed ? "changed" : "current"}`).text_sm().text_color(theme().colors.muted_foreground)
        .child(this.changed ? text.changed : (this.connected ? text.watching : text.offline)))
      .child(div().text_sm().text_color(theme().colors.muted_foreground).child(path))
      .child(div().id("summary-report").flex_1().min_h_0().overflow_y_scroll().text_sm()
        .child(this.status ? report(this.status, this.include, text) : ""));
  }
}
