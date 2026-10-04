import test from "node:test";
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import vm from "node:vm";
import {messages} from "../dev.sailry.platform/desktop/locales.js";

const plain = value => JSON.parse(JSON.stringify(value));
const source = (await readFile(new URL("../dev.sailry.platform/desktop/view.js", import.meta.url), "utf8"))
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const controller = (await readFile(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8"))
  .replace(/^import .*;\n/gm, "").replace(/^export default /gm, "");
const control = vm.runInNewContext(`${controller}\nReminders.prototype.control`, {View:class {}});

function fixture(items = []) {
  const nodes = new Map(), calls = [], cx = {notify() {}};
  function element(kind = "div", id = null, props = {}) {
    const value = {kind, selector:id, props, items:[], style:{}};
    const proxy = new Proxy(value, {get(target, key) {
      if (key === "id") return id => { target.selector = id; nodes.set(id, proxy); return proxy; };
      if (key === "child") return value => { target.items.push(value); return proxy; };
      if (key === "children") return values => { target.items.push(...values); return proxy; };
      if (key === "item") return (label, callback) => { target.items.push({label, callback}); return proxy; };
      if (key in target) return target[key];
      if (key === "on_click" || key === "on_change") return callback => { target[key] = callback; return proxy; };
      return (...args) => { target.style[key] = args; return proxy; };
    }});
    if (id) nodes.set(id, proxy);
    return proxy;
  }
  const constructors = Object.fromEntries(["Button", "Checkbox", "Tab", "TabBar"]
    .map(kind => [kind, function(id, label) { return element(kind, id, {label}); }]));
  const adapters = Object.fromEntries(["CardColumn", "CardList", "CardRow", "CardSummary", "EmptyState", "IconButton", "Modal", "SelectField", "SettingsPage", "Toggle"]
    .map(kind => [kind, {new:(id, props) => element(kind, id, props)}]));
  const {render} = vm.runInNewContext(`${source}\n({render})`, {
    div:() => element(), ...constructors, ...adapters,
    theme:() => ({colors:{foreground:"foreground", muted_foreground:"muted", destructive:"danger"}}),
    editor:() => [element("editor"),element("footer")], window:{viewport_size:() => ({width:1200})},
  });
  const view = {text:messages("en"), items, completed:null, loading:false,
    projectFilter:"all", catalog:{projects:[{id:"project", name:"Current"}],worktrees:[]},
    busy:false, pending:null, error:null, editing:null, deleting:null, dialogId:0,
    refresh:() => calls.push(["refresh"]), edit:item => calls.push(["edit", item]),
    perform:(handler, input) => calls.push([handler, input]), close:() => calls.push(["close"]),
    projectName(id) { return id === null ? this.text.noProject : this.catalog.projects.find(project => project.id === id)?.name ?? this.text.unavailableProject; },
    control,
  };
  return {view, nodes, calls, cx, render() { nodes.clear(); return render(view); }};
}

test("page uses the centered settings layout and native toolbar controls", () => {
  const setup = fixture();
  setup.render();
  const {nodes} = setup, body = nodes.get("reminders");
  assert.equal(body.kind, "SettingsPage");
  assert.deepEqual(plain(body.props), {title:"Reminders", description:"To-dos and scheduled reminders"});
  assert.equal(nodes.get("reminders-items").kind, "CardList");
  const toolbar = nodes.get("reminders-toolbar");
  assert.deepEqual(plain(toolbar.style.items_center), []);
  assert.deepEqual(plain(nodes.get("reminders-filters").style.items_center), []);
  assert.deepEqual(plain(nodes.get("reminders-filters").style.flex_1), []);
  assert.equal(toolbar.items[0].items[0].kind, "TabBar");
  assert.equal(toolbar.items[0].items[0].selector, "reminders-filter");
  assert.deepEqual(plain(toolbar.items[0].items[0].style.variant), ["segmented"]);
  assert.deepEqual(plain(toolbar.items[0].items[0].style.selected_index), [0]);
  assert.deepEqual(toolbar.items[0].items[0].items.map(tab => tab.style.label[0]), ["All", "Pending", "Completed"]);
  assert.equal(toolbar.items[0].items[1].items[0].kind, "SelectField");
  assert.equal(toolbar.items[0].items[1].items[0].selector, "reminders-project-filter");
  assert.equal(toolbar.items[1].selector, "reminder-create");
  assert.equal(nodes.has("reminders-refresh"), false);
  assert.equal(nodes.has("reminders-reload"), false);
  assert.equal(nodes.get("reminders-empty").kind, "EmptyState");
  assert.deepEqual(plain(nodes.get("reminders-empty").props), {variant:"card", icon:"bell", label:"No reminders"});
  nodes.get("reminder-create").on_click({}, setup.cx);
  assert.deepEqual(setup.calls, [["edit", null]]);
});

test("load failure keeps the page without a retry bar", () => {
  const setup = fixture(); setup.view.error = "loadFailed"; setup.render();
  assert.equal(setup.nodes.has("reminders-refresh"), false);
  assert.equal(setup.nodes.has("reminders-reload"), false);
  assert.ok(setup.nodes.has("reminders-items"));
  assert.deepEqual(setup.calls, []);
});

test("form delegates its heading, close guard and body/footer to the shared modal", () => {
  const setup = fixture(); setup.view.editing = {draft:{}}; setup.render();
  const dialog = setup.nodes.get("reminder-dialog-0");
  assert.deepEqual(plain(dialog.props), {open:true,dismissable:true,form:true,title:"New reminder",width:480});
  assert.deepEqual(dialog.items.map(item=>item.kind), ["editor","footer"]);
  setup.view.editing.draft.id="saved"; setup.view.busy=true; setup.render();
  assert.equal(setup.nodes.get("reminder-dialog-0").props.title,"Edit");
  assert.equal(setup.nodes.get("reminder-dialog-0").props.dismissable,false);
  setup.view.busy=false; setup.view.pending="request"; setup.render();
  assert.equal(setup.nodes.get("reminder-dialog-0").props.dismissable,true);
});

test("loading retains its placeholder without an empty reminder card", () => {
  const setup=fixture();setup.view.loading=true;setup.render();
  assert.equal(setup.nodes.has("reminders-empty"),false);
  assert.ok(setup.nodes.get("reminders-items").items.some(item=>item.items.includes(setup.view.text.loading)));
});

test("background refresh preserves the loaded empty card", () => {
  const setup=fixture();setup.view.loaded=true;setup.view.loading=true;setup.render();
  assert.ok(setup.nodes.has("reminders-empty"));
  assert.ok(!setup.nodes.get("reminders-items").items.some(item=>item.items.includes(setup.view.text.loading)));
});

test("admitted actions do not insert recovery rows while still pending", () => {
  const setup=fixture();setup.view.busy=true;setup.view.pending="in-flight";setup.render();
  assert.equal(setup.nodes.has("reminder-retry"),false);
  assert.equal(setup.nodes.has("reminder-dismiss"),false);
  assert.ok(setup.nodes.has("reminders-toolbar") && setup.nodes.has("reminders-items"));
});

test("completion tabs preserve reminder actions inside shared cards", () => {
  const pending = {id:"pending", project:null, revision:"4", title:"Pending item", message:"Keep this note", due_ms:null, notified_ms:null, completed:false};
  const completed = {...pending, id:"completed", title:"Completed item", completed:true};
  const setup = fixture([pending, completed]);
  setup.render();
  assert.ok(setup.nodes.has("reminder-card-pending"));
  assert.ok(setup.nodes.has("reminder-card-completed"));
  assert.equal(setup.nodes.get("reminder-card-pending").kind, "CardRow");
  assert.deepEqual(plain(setup.nodes.get("reminder-card-pending").props), {row_id:"reminder-row-pending"});
  setup.view.control({id:"reminder-complete-pending",value:true}, setup.cx);
  assert.deepEqual(plain(setup.calls[0]), ["save", {...pending, completed:true}]);
  setup.nodes.get("reminders-filter").on_change(1, setup.cx);
  setup.render();
  assert.ok(setup.nodes.has("reminder-card-pending"));
  assert.equal(setup.nodes.has("reminder-card-completed"), false);
  setup.nodes.get("reminders-filter").on_change(2, setup.cx);
  setup.render();
  assert.equal(setup.nodes.has("reminder-card-pending"), false);
  assert.ok(setup.nodes.has("reminder-card-completed"));
  assert.deepEqual(plain(setup.nodes.get("reminders-filter").style.selected_index), [2]);
  setup.view.control({id:"reminder-edit-completed"}, setup.cx);
  assert.deepEqual(setup.calls[1], ["edit", completed]);
  setup.view.control({id:"reminder-delete-completed"}, setup.cx);
  assert.equal(setup.view.deleting, completed);
  setup.render();
  setup.nodes.get("reminder-delete-confirm").on_click({}, setup.cx);
  assert.deepEqual(plain(setup.calls[2]), ["remove", {id:"completed", revision:"4"}]);
  setup.nodes.get("reminders-filter").on_change(0, setup.cx); setup.render();
  assert.ok(setup.nodes.has("reminder-card-pending") && setup.nodes.has("reminder-card-completed"));
});

test("cards delegate ordered column roles and preserve native actions", () => {
  const item = {id:"row", project:"project", revision:"1", title:"Long reminder 中文 🙂", message:"Preserve this note in the editor",
    due_ms:null, notified_ms:null, completed:false};
  const setup = fixture([item]); setup.render();
  const row = setup.nodes.get("reminder-card-row");
  assert.equal(row.kind, "CardRow");
  assert.deepEqual(plain(row.props), {row_id:"reminder-row-row"});
  assert.deepEqual(row.items.map(child => child.kind), ["CardSummary","CardColumn","CardColumn"]);
  assert.deepEqual(row.items.map(child => child.selector), ["reminder-summary-row","reminder-time-row","reminder-actions-row"]);
  assert.deepEqual(row.items.slice(1).map(child => child.props.variant), ["wide_metadata","actions"]);
  assert.deepEqual(plain(row.items[0].props), {title:item.title,subtitle:"Current",icon:"icons/folder.svg"});
  const complete = setup.nodes.get("reminder-complete-row");
  assert.equal(complete.kind, "Toggle");
  assert.deepEqual(plain(complete.props), {variant:"checkbox",label:"Complete: Long reminder 中文 🙂",checked:false,disabled:false});
  assert.equal(row.items[0].items[0], complete);
  assert.deepEqual(plain(setup.nodes.get("reminder-time-row").items), [""]);
  const actions = setup.nodes.get("reminder-actions-row");
  assert.deepEqual(plain(actions.props), {variant:"actions"});
  assert.deepEqual(actions.items.map(child => child.selector), ["reminder-edit-row","reminder-delete-row"]);
  assert.deepEqual(actions.items.map(child => child.kind), ["IconButton","IconButton"]);
  assert.deepEqual(plain(actions.items.map(child => child.props)), [
    {icon:"reicon:newicons/edit",label:"Edit",disabled:false},
    {icon:"reicon:ui/trash2",label:"Delete",disabled:false},
  ]);
  assert.ok(actions.items.every(child => !Object.hasOwn(child,"on_click")));
  assert.equal(JSON.stringify(plain(row.items)).includes(item.message), false);
  setup.view.control({id:"reminder-edit-row"}, setup.cx);
  assert.equal(setup.calls[0][1].message, item.message);
});

test("completed cards inherit a whole-row strike without changing native controls", () => {
  const item = {id:"done", project:"project", revision:"1", title:"Finished", message:"Original note", due_ms:null, notified_ms:0, completed:true};
  const setup = fixture([item, {...item,id:"pending",completed:false}]); setup.render();
  const row = setup.nodes.get("reminder-card-done");
  assert.deepEqual(plain(row.style.line_through), []);
  assert.equal(setup.nodes.get("reminder-card-pending").style.line_through, undefined);
  assert.equal(row.items[0].kind, "CardSummary");
  assert.deepEqual(row.items.slice(1).map(child => child.props.variant), ["wide_metadata","actions"]);
  assert.equal(setup.nodes.get("reminder-complete-done").props.checked, true);
  assert.equal(setup.nodes.get("reminder-edit-done").props.label, "Edit");
  setup.view.control({id:"reminders-project-filter",value:"project"},setup.cx);
  setup.nodes.get("reminders-filter").on_change(2,setup.cx); setup.render();
  assert.ok(setup.nodes.has("reminder-card-done"));
  assert.equal(setup.nodes.has("reminder-card-pending"),false);
  setup.nodes.get("reminders-filter").on_change(0,setup.cx); setup.render();
  assert.ok(setup.nodes.has("reminder-card-done") && setup.nodes.has("reminder-card-pending"));
  assert.equal(setup.view.projectFilter,"project");
});

test("missing times remain empty and notification labels have no leading separator", () => {
  const item={id:"row",project:null,revision:"1",title:"Reminder",message:"",due_ms:null,notified_ms:null,completed:false};
  const setup=fixture([item]);
  for(const locale of ["en","zh"]) {
    setup.view.text=messages(locale);
    for(const [due,notified] of [[null,null],[null,0],[1792000000000,null],[1792000000000,0]]) {
      setup.view.items=[{...item,due_ms:due,notified_ms:notified}];setup.render();
      const expected=[due===null?"":new Date(due).toLocaleString(),notified===null?"":setup.view.text.notified].filter(Boolean).join(" · ");
      assert.deepEqual(plain(setup.nodes.get("reminder-time-row").items),[expected]);
      assert.equal(expected.startsWith(" · "),false);
    }
  }
});

test("uncertain actions retain retry and dismiss controls without enabling writes", () => {
  const item = {id:"item", project:null, revision:"3", title:"Saved", message:"", due_ms:null, notified_ms:null, completed:false};
  const setup = fixture([item]);
  setup.view.pending = "original-request";
  setup.view.error = "unknown";
  setup.render();
  assert.deepEqual(plain(setup.nodes.get("reminder-create").style.disabled), [true]);
  for (const id of ["reminder-edit-item", "reminder-delete-item"])
    assert.equal(setup.nodes.get(id).props.disabled, true);
  assert.equal(setup.nodes.get("reminder-complete-item").props.disabled, true);
  setup.view.control({id:"reminder-complete-item",value:true},setup.cx);
  setup.view.control({id:"reminder-delete-item"},setup.cx);
  assert.deepEqual(setup.calls, []);
  assert.equal(setup.view.deleting,null);
  setup.nodes.get("reminder-retry").on_click({}, setup.cx);
  setup.nodes.get("reminder-dismiss").on_click({}, setup.cx);
  assert.deepEqual(setup.calls, [[null, null], ["close"]]);
  assert.equal(setup.view.pending, "original-request");
});

test("desktop navigation and project tool share the host package without contract changes", async () => {
  const manifest = JSON.parse(await readFile(new URL("../plugin.json", import.meta.url), "utf8"));
  const extension = manifest.extensions["dev.sailry.platform"];
  assert.equal(extension.scope, undefined);
  assert.equal(extension.desktop.navigation_options.target, undefined);
  assert.equal(extension.api_version, "v1");
  assert.equal(manifest.version, "0.1.0");
  assert.deepEqual(extension.actions, ["storage.read", "storage.write", "dispatch.manage", "notifications.publish", "projects.read"]);
  assert.equal(extension.tools.length, 1);
  assert.deepEqual(extension.tools[0].contexts, ["workspace"]);
  assert.deepEqual(extension.tools[0].handler.flow.operations, ["storage.transaction"]);
  assert.equal("project" in extension.tools[0].handler.parameters.properties, false);
  assert.equal(extension.host.turn.initialize, "initialize");
  assert.equal(messages("zh-CN").pageTitle, "待办提醒");
  assert.equal(messages("zh-CN").description, "管理待办事项与定时提醒");
});

test("captured tool display keeps localized action and target prompts out of the payload", async () => {
  const manifest = JSON.parse(await readFile(new URL("../plugin.json", import.meta.url), "utf8"));
  const tool = manifest.extensions["dev.sailry.platform"].tools[0];
  assert.equal(tool.presentation,"summary");
  assert.deepEqual(tool.display.input,{summary:{path:"/title"}});
  assert.equal(tool.display.output,undefined);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.message.label),[
    "Create reminder %{title}","Update reminder %{title}","Update reminder %{id}","Delete reminder %{id}",
  ]);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.message.locales["zh-CN"]),[
    "创建提醒 %{title}","更新提醒 %{title}","更新提醒 %{id}","删除提醒 %{id}",
  ]);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.values),[
    {title:{source:"arguments",path:"/title"}},{title:{source:"arguments",path:"/title"}},
    {id:{source:"arguments",path:"/id"}},{id:{source:"arguments",path:"/id"}},
  ]);
  const matches = (prompt,input) => prompt.when.all.every(condition => {
    const value = input[condition.value.path.slice(1)] ?? null;
    return (value === condition.equals) !== !!condition.negate;
  });
  for (const [input,index] of [[{action:"create",title:"New"},0],[{action:"update",title:"Changed"},1],
    [{action:"update",id:"saved"},2],[{action:"delete",id:"saved"},3]]) {
    assert.deepEqual(tool.display.approval.map((prompt,index)=>matches(prompt,input) ? index : null).filter(index=>index!==null),[index]);
  }
  assert.ok(tool.display.approval.every(prompt=>!matches(prompt,{action:"list"})));
});

test("project filter keeps all, unassigned and unavailable associations distinct", () => {
  const item = {id:"current", project:"project", revision:"1", title:"Current", message:"", due_ms:null, notified_ms:null, completed:false};
  const setup = fixture([item, {...item,id:"global",project:null}, {...item,id:"missing",project:"removed"}]);
  setup.render();
  assert.equal(setup.nodes.get("reminders-project-filter").kind, "SelectField");
  assert.deepEqual(plain(setup.nodes.get("reminders-project-filter").props.items).map(item => item.label),
    ["All projects", "No project", "Current", "Unavailable project"]);
  assert.equal(setup.nodes.get("reminder-summary-current").props.subtitle, "Current");
  assert.equal(setup.nodes.get("reminder-summary-missing").props.subtitle, "Unavailable project");
  setup.view.control({id:"reminders-project-filter",value:"none"}, setup.cx);
  setup.render();
  assert.ok(setup.nodes.has("reminder-card-global"));
  assert.equal(setup.nodes.has("reminder-card-current"), false);
  setup.view.control({id:"reminders-project-filter",value:"project"}, setup.cx);
  setup.render();
  assert.ok(setup.nodes.has("reminder-card-current"));
  assert.equal(setup.nodes.has("reminder-card-global"), false);
  setup.view.control({id:"reminders-project-filter",value:"removed"}, setup.cx);
  setup.render();
  assert.ok(setup.nodes.has("reminder-card-missing"));
  setup.nodes.get("reminder-create").on_click({}, setup.cx);
  assert.deepEqual(setup.calls, [["edit", null]]);
  assert.equal(setup.view.projectFilter, "removed");
  setup.view.control({id:"reminders-project-filter",value:"not-a-choice"}, setup.cx);
  assert.equal(setup.view.projectFilter, "removed");
  setup.view.control({id:"reminders-project-filter",value:"all"}, setup.cx); setup.render();
  assert.equal(setup.nodes.get("reminders-project-filter").props.selected, "all");
  assert.ok(setup.nodes.has("reminder-card-current") && setup.nodes.has("reminder-card-global") && setup.nodes.has("reminder-card-missing"));
});
