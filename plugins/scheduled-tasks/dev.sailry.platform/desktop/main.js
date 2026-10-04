import {View} from "gpui-kit";
import {context} from "sailry";
import {prepareRequest, completeRequest, forgetRequest, nextChange, readProjectCatalog, listModels, resolveSessionModel} from "sailry/sdk";
import {modal_closed,nextControlEvent,toast} from "sailry/ui";
import {list, history, states} from "../host/management.js";
import {messages} from "./locales.js";
import {editor, release, value, effortKey} from "./editor.js";
import {render} from "./view.js";

const uncertain = code => ["unavailable", "busy", "outcome_unknown", "internal", "cancelled"].includes(code);
function unwrap(result) { if (result.Err) throw result.Err; return result.Ok; }
export default class ScheduledTasks extends View {
  init(_props, cx) {
    this.text = messages(JSON.parse(context()).locale);
    this.items = []; this.history = []; this.pages = 1; this.more = false; this.tab = 0;
    this.catalog = {projects:[],worktrees:[]}; this.loading = false; this.loaded = false; this.dirty = false;
    this.models = []; this.modelsError = false;
    this.busy = false; this.pending = null; this.error = null;
    this.editing = null; this.deleting = null; this.dialogId = 0;
    cx.spawn(async cx => {
      let seen = "";
      try { while (true) {
        const change = await nextChange(seen); seen = change.cursor;
        if (change.connected) this.refresh(cx);
      } } catch (_) { /* View release ends observation. */ }
    });
    cx.spawn(async cx => {
      try { while (true) this.control(await nextControlEvent(),cx); }
      catch (_) { /* View release ends its native controls. */ }
    });
    cx.spawn(async cx => {
      try { while (true) {
        if (await modal_closed() === `task-dialog-${this.dialogId}`) this.close(cx);
      } } catch (_) { /* View release ends observation. */ }
    });
  }
  refresh(cx) {
    if (this.loading) { this.dirty = true; return; }
    this.loading = true; cx.notify();
    cx.spawn(async cx => {
      try { do {
        this.dirty = false;
        const items = []; let after = null;
        do {
          const page = unwrap(await list({after}));
          items.push(...page.items); after = page.after;
        } while (after !== null);
        const runs = []; let before = null;
        for (let index = 0; index < this.pages; index++) {
          const page = unwrap(await history({before}));
          runs.push(...page.items); before = page.next_before;
          if (before === null) break;
        }
        const status = items.length ? await states() : {};
        this.catalog = await readProjectCatalog();
        this.items = items.map(item => ({...item,status:status[item.id] ?? null}));
        this.history = runs; this.more = before !== null; this.loaded = true;
        if (this.error === "loadFailed") this.error = null;
        try { this.models = (await listModels()).models.filter(model => model.kind === "provider"); this.modelsError = false; }
        catch (_) { this.modelsError = true; this.report("modelsFailed"); }
        cx.notify();
      } while (this.dirty); }
      catch (_) { this.report("loadFailed"); }
      finally { this.loading = false; cx.notify(); }
    });
  }
  projectName(id) {
    return id ? this.catalog.projects.find(project => project.id === id)?.name ?? this.text.unavailable : this.text.noProject;
  }
  control(event,cx) {
    const item = this.items.find(item => ["enabled","run","edit","delete"].some(action => event.id === `task-${action}-${item.id}`));
    if (item) {
      if (this.busy || this.pending) return;
      if (event.id === `task-enabled-${item.id}` && typeof event.value === "boolean") this.perform("save", {...item,enabled:event.value},cx);
      else if (event.id === `task-run-${item.id}`) this.perform("run", {id:item.id},cx);
      else if (event.id === `task-edit-${item.id}`) this.edit(item,cx);
      else if (event.id === `task-delete-${item.id}`) { this.deleting = item; this.error = null; this.dialogId++; cx.notify(); }
      return;
    }
    const editing = this.editing;
    if (!editing || this.busy || this.pending) return;
    if (event.id === "task-timing" && ["once","every"].includes(event.value)) editing.repeat = event.value === "every";
    else if (event.id === "task-project") {
      const project = event.value === "none" ? null : this.catalog.projects.find(project => project.id === event.value)?.id;
      if (project === undefined) return;
      editing.draft.project = project; editing.draft.worktree = null;
    } else if (event.id === "task-worktree") {
      const worktree = event.value === "main" ? null : this.catalog.worktrees
        .find(tree => tree.id === event.value && tree.project === editing.draft.project)?.id;
      if (worktree === undefined || !editing.draft.project) return;
      editing.draft.worktree = worktree;
    } else if (event.id === "task-model") return this.selectModel(event.value?.model,event.value?.effort,cx);
    else if (event.id === "task-strength") {
      const model = this.models.find(model => model.id === editing.model);
      const effort = model?.efforts.find(effort => effortKey(effort) === event.value);
      if (effort === undefined || editing.modelLoading) return;
      return this.selectModel(editing.model,effort,cx);
    } else return;
    cx.notify();
  }
  selectModel(model,effort,cx) {
    const editing = this.editing;
    if (!editing || this.busy || this.pending || !this.models.some(choice => choice.id === model)) return;
    const version = ++editing.modelVersion;
    editing.model = model; editing.modelLoading = true; cx.notify();
    cx.spawn(async cx => {
      try {
        const config = await resolveSessionModel(model,effort,editing.draft.config);
        if (this.editing !== editing || editing.modelVersion !== version) return;
        editing.draft.config = config; editing.model = `${config.provider}/${config.model}`;
      } catch (_) {
        if (this.editing !== editing || editing.modelVersion !== version) return;
        editing.model = editing.draft.config ? `${editing.draft.config.provider}/${editing.draft.config.model}` : null;
        this.report("modelFailed");
      } finally {
        if (this.editing === editing && editing.modelVersion === version) { editing.modelLoading = false; cx.notify(); }
      }
    });
  }
  report(key) { this.error=key;toast({id:'scheduled-tasks-error',message:this.text[key],kind:'error'}); }
  edit(item, cx) {
    if (this.busy || this.pending) return;
    this.close(cx); this.editing = editor(item, this.text); this.dialogId++; cx.notify();
  }
  close(cx) {
    if (this.busy) return;
    if (this.pending) { forgetRequest(this.pending); this.pending = null; this.refresh(cx); }
    if (this.editing) release(this.editing);
    this.editing = null; this.deleting = null; this.error = null; cx.notify();
  }
  save(cx) {
    if (this.pending) return this.perform(null, null, cx);
    if (this.editing?.modelLoading) return;
    try { this.perform("save", value(this.editing), cx); }
    catch (error) { this.report(["required", "modelRequired", "invalidTime", "invalidInterval"].includes(error.message) ? error.message : "failed"); cx.notify(); }
  }
  perform(handler, input, cx) {
    if (this.busy) return;
    this.busy = true; this.error = null; cx.notify();
    cx.spawn(async cx => {
      try {
        if (!this.pending) this.pending = prepareRequest({kind:"call_plugin",data:{handler,input}});
        const receipt = await completeRequest(this.pending);
        if (receipt.Err && uncertain(receipt.Err.code)) { this.error = "unknown"; return; }
        const fault = receipt.Err ?? receipt.Ok?.data?.Err;
        if (fault) this.error = fault.code === "revision_conflict" ? "conflict"
          : fault.code === "not_configured" ? "notConfigured" : "failed";
        forgetRequest(this.pending); this.pending = null; this.busy = false;
        if (!this.error) this.close(cx);
        this.refresh(cx);
      } catch (_) { this.error = this.pending ? "unknown" : "failed"; }
      finally { this.busy = false;if(this.error)this.report(this.error);cx.notify(); }
    });
  }
  render(cx) { return render(this); }
}
