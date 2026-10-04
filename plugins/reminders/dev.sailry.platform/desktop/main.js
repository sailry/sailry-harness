import { View } from "gpui-kit";
import { context } from "sailry";
import { prepareRequest, completeRequest, forgetRequest, nextChange, readProjectCatalog } from "sailry/sdk";
import { modal_closed,nextControlEvent,toast } from "sailry/ui";
import { messages } from "./locales.js";
import { editor, release, value } from "./editor.js";
import { render } from "./view.js";
import {page} from "../host/records.js";

const uncertain = code => ["unavailable", "busy", "outcome_unknown", "internal", "cancelled"].includes(code);
export default class Reminders extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.items = []; this.completed = null; this.loading = false; this.loaded = false; this.dirty = false;
    this.catalog = {projects:[],worktrees:[]}; this.projectFilter = "all";
    this.busy = false; this.pending = null; this.error = null;
    this.editing = null; this.deleting = null; this.dialogId = 0;
    cx.spawn(async cx => {
      let seen = "";
      try {
        while (true) {
          const change = await nextChange(seen);
          seen = change.cursor;
          if (change.connected) this.refresh(cx);
        }
      } catch (_) { /* View release ends observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        const id = await modal_closed();
        if (id === `reminder-dialog-${this.dialogId}`) this.close(cx);
      } } catch (_) { /* View release ends observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) this.control(await nextControlEvent(), cx); }
      catch (_) { /* View release ends its native controls. */ }
    });
  }
  refresh(cx) {
    if (this.loading) { this.dirty = true; return; }
    this.loading = true; cx.notify();
    cx.spawn(async cx => {
      try {
        do {
          this.dirty = false;
          const items = []; let after = null;
          do {
            const batch = await page({after});
            items.push(...batch.items); after = batch.after;
          } while (after !== null);
          items.sort((a, b) => (a.due_ms ?? Number.MAX_SAFE_INTEGER) - (b.due_ms ?? Number.MAX_SAFE_INTEGER)
            || a.title.localeCompare(b.title));
          this.catalog = await readProjectCatalog(); this.items = items; this.loaded = true;
          if (this.error === "loadFailed") this.error = null;
          cx.notify();
        } while (this.dirty);
      } catch (_) { this.report("loadFailed"); }
      finally { this.loading = false; cx.notify(); }
    });
  }
  projectName(id) {
    return id === null ? this.text.noProject : this.catalog.projects.find(project => project.id === id)?.name ?? this.text.unavailableProject;
  }
  control(event, cx) {
    const item = this.items.find(item => event.id === `reminder-complete-${item.id}`);
    if (item) {
      if (!this.busy && !this.pending && typeof event.value === "boolean") this.perform("save", {...item, completed:event.value}, cx);
      return;
    }
    const editing = this.items.find(item => event.id === `reminder-edit-${item.id}`);
    if (editing) { this.edit(editing, cx); return; }
    const deleting = this.items.find(item => event.id === `reminder-delete-${item.id}`);
    if (deleting) {
      if (!this.busy && !this.pending) {
        this.deleting = deleting; this.error = null; this.dialogId++; cx.notify();
      }
      return;
    }
    if (event.id === "reminders-project-filter") {
      if (event.value === "all") this.projectFilter = "all";
      else if (event.value === "none") this.projectFilter = null;
      else if (this.catalog.projects.some(project => project.id === event.value)
          || this.items.some(item => item.project === event.value)) this.projectFilter = event.value;
      else return;
    } else if (event.id === "reminder-project" && this.editing && !this.busy && !this.pending) {
      if (event.value === "none") this.editing.draft.project = null;
      else if (this.catalog.projects.some(project => project.id === event.value)) this.editing.draft.project = event.value;
      else return;
    } else return;
    cx.notify();
  }
  report(key) { this.error=key;toast({id:'reminders-error',message:this.text[key],kind:'error'}); }
  edit(item, cx) {
    if (this.busy || this.pending) return;
    this.close(cx); this.editing = editor(item, this.text); this.dialogId++; cx.notify();
  }
  close(cx) {
    if (this.busy) return;
    // Closing an uncertain draft releases only its UI handle, never its Node work.
    if (this.pending) { forgetRequest(this.pending); this.pending = null; this.refresh(cx); }
    if (this.editing) release(this.editing);
    this.editing = null; this.deleting = null; this.error = null; cx.notify();
  }
  save(cx) {
    if (this.pending) return this.perform(null, null, cx);
    try { this.perform("save", value(this.editing), cx); }
    catch (error) { this.report(["required", "invalidTime"].includes(error.message) ? error.message : "failed"); cx.notify(); }
  }
  perform(handler, input, cx) {
    if (this.busy) return;
    this.busy = true; this.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        if (!this.pending) this.pending = prepareRequest({kind:"call_plugin",data:{handler,input}});
        const receipt = await completeRequest(this.pending);
        if (receipt.Err) {
          if (uncertain(receipt.Err.code)) { this.error = "unknown"; return; }
          this.error = "failed";
        } else {
          const result = receipt.Ok.data;
          if (result.Err) this.error = result.Err.code === "revision_conflict" ? "conflict"
            : result.Err.code === "not_found" ? "projectUnavailable" : "failed";
        }
        forgetRequest(this.pending); this.pending = null;
        this.busy = false;
        if (!this.error) this.close(cx);
        this.refresh(cx);
      } catch (_) { this.error = this.pending ? "unknown" : "failed"; }
      finally { this.busy = false;if(this.error)this.report(this.error);cx.notify(); }
    });
  }
  render(cx) { return render(this); }
}
