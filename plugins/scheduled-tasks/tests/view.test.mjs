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
const control = vm.runInNewContext(`${controller}\nScheduledTasks.prototype.control`, {View:class {}});

function fixture() {
  const nodes = new Map(), calls = [], cx = {notify() {}};
  function element(kind = "div", id = null, props = {}) {
    const value = {kind, selector:id, props, items:[], style:{}};
    const proxy = new Proxy(value, {get(target, key) {
      if (key === "id") return id => { target.selector = id; nodes.set(id, proxy); return proxy; };
      if (key === "child") return value => { target.items.push(value); return proxy; };
      if (key === "children") return values => { target.items.push(...values); return proxy; };
      if (key in target) return target[key];
      if (key === "on_click" || key === "on_change") return callback => { target[key] = callback; return proxy; };
      return (...args) => { target.style[key] = args; return proxy; };
    }});
    if (id) nodes.set(id, proxy);
    return proxy;
  }
  const constructors = Object.fromEntries(["Button", "Switch", "Tab", "TabBar"]
    .map(kind => [kind, function(id) { return element(kind, id); }]));
  const adapters = Object.fromEntries(["CardColumn", "CardList", "CardRow", "CardSummary", "EmptyState", "IconButton", "Modal", "SettingsPage", "Toggle", "Tooltip"]
    .map(kind => [kind, {new:(id, props) => element(kind, id, props)}]));
  const {render} = vm.runInNewContext(`${source}\n({render})`, {
    div:() => element(), ...constructors, ...adapters,
    theme:() => ({colors:{foreground:"foreground", muted_foreground:"muted", destructive:"danger", success:"success"}}),
    openSession:session => calls.push(["open", session]),
    editor:() => [element("editor"),element("footer")], window:{viewport_size:() => ({width:1200})},
  });
  const view = {text:messages("en"), tab:0, items:[], history:[], pages:1, more:false, loading:false,
    busy:false, pending:null, error:null, editing:null, deleting:null, dialogId:0,
    projectName:() => "Project", refresh:() => calls.push(["refresh"]),
    edit:item => calls.push(["edit", item]), perform:(handler, input) => calls.push([handler, input]),
    close:() => calls.push(["close"]),
    control,
  };
  return {view, nodes, calls, cx, render() { nodes.clear(); return render(view); }};
}

test("page matches settings layout and separates empty Tasks and Runs tabs", () => {
  const setup = fixture();
  setup.render();
  const {nodes} = setup, body = nodes.get("tasks");
  assert.equal(body.kind, "SettingsPage");
  assert.deepEqual(plain(body.props), {title:"Scheduled tasks", description:"Run tasks on a schedule"});
  assert.equal("overflow_y_scroll" in nodes.get("scheduled-tasks-page").style, false);
  const toolbar = nodes.get("tasks-controls");
  assert.deepEqual(toolbar.items.map(child => child.kind), ["TabBar", "Button"]);
  assert.equal(toolbar.items[1].selector, "task-create");
  assert.equal(nodes.has("tasks-refresh"), false);
  assert.equal(nodes.get("tasks-items").kind, "CardList");
  assert.equal(nodes.get("tasks-empty").kind, "EmptyState");
  assert.deepEqual(plain(nodes.get("tasks-empty").props), {variant:"card", icon:"calendar", label:"No tasks"});
  assert.equal(nodes.has("tasks-runs"), false);
  nodes.get("tasks-tabs").on_change(1, setup.cx);
  setup.render();
  assert.equal(setup.view.tab, 1);
  assert.deepEqual(plain(nodes.get("tasks-tabs").style.selected_index), [1]);
  assert.equal(nodes.has("tasks-items"), false);
  assert.equal(nodes.get("tasks-runs").kind, "CardList");
  assert.equal(nodes.get("tasks-runs-empty").kind, "EmptyState");
  assert.deepEqual(plain(nodes.get("tasks-runs-empty").props), {variant:"card", icon:"calendar", label:"No records"});
});

test("loading retains its placeholder without an empty card in either tab", () => {
  const setup=fixture();setup.view.loading=true;
  for(const tab of [0,1]) {
    setup.view.tab=tab;setup.render();
    assert.equal(setup.nodes.has("tasks-empty"),false);
    assert.equal(setup.nodes.has("tasks-runs-empty"),false);
    const list=setup.nodes.get(tab?"tasks-runs":"tasks-items");
    assert.ok(list.items.some(item=>item.items.includes(setup.view.text.loading)));
  }
});

test("failed reads retain both tabs without a retry bar and keep pending action recovery", () => {
  const setup = fixture();
  setup.view.error = "loadFailed";
  for (const tab of [0,1]) {
    setup.view.tab = tab; setup.render();
    assert.equal(setup.nodes.has("tasks-refresh"), false);
    assert.ok(setup.nodes.has(tab ? "tasks-runs" : "tasks-items"));
    assert.equal(setup.view.tab, tab);
  }
  setup.view.error = "unknown"; setup.view.pending = "original-request"; setup.render();
  assert.ok(setup.nodes.has("task-retry"));
  assert.ok(setup.nodes.has("task-dismiss"));
  assert.deepEqual(setup.calls, []);
});

test("background refresh retains empty cards and pending writes retain toolbar identity", () => {
  const setup=fixture();setup.view.loaded=true;setup.view.loading=true;
  for(const tab of [0,1]) {
    setup.view.tab=tab;setup.render();
    assert.ok(setup.nodes.has(tab ? "tasks-runs-empty" : "tasks-empty"));
    const list=setup.nodes.get(tab ? "tasks-runs" : "tasks-items");
    assert.ok(!list.items.some(item=>item.items.includes(setup.view.text.loading)));
  }
  setup.view.busy=true;setup.view.pending="in-flight";setup.render();
  assert.ok(setup.nodes.has("tasks-controls"));
  assert.equal(setup.nodes.has("task-retry"),false);
  assert.equal(setup.nodes.has("task-dismiss"),false);
});

test("Tasks retains per-task actions while Records retains session and stop routes", () => {
  const setup = fixture(), item = {id:"task", name:"Review", prompt:"Keep this task", revision:"9", project:"project", queue:"default", enabled:true, next_ms:null};
  setup.view.items = [item];
  setup.view.history = [
    {name:"Queued review", job:{id:"queued", created_ms:0, status:"queued"}},
    {name:"Running review", job:{id:"running", created_ms:0, status:"running"}, session:"original-session", turn:"original-turn"},
  ];
  setup.view.more = true;
  setup.render();
  assert.equal(setup.nodes.get("task-card-task").kind, "CardRow");
  assert.deepEqual(plain(setup.nodes.get("task-card-task").props), {row_id:"task-row-task"});
  assert.equal(setup.nodes.has("task-execution-running"), false);
  setup.view.control({id:"task-enabled-task",value:false},setup.cx);
  setup.view.control({id:"task-run-task"}, setup.cx);
  assert.deepEqual(plain(setup.calls), [
    ["save", {...item, enabled:false}], ["run", {id:"task"}],
  ]);
  setup.nodes.get("tasks-tabs").on_change(1, setup.cx);
  setup.render();
  assert.equal(setup.nodes.has("task-card-task"), false);
  assert.equal(setup.nodes.has("task-queue-default"), false);
  assert.equal(setup.nodes.get("task-execution-running").kind, "CardRow");
  setup.nodes.get("task-open-running").on_click({}, setup.cx);
  setup.nodes.get("task-stop-queued").on_click({}, setup.cx);
  setup.nodes.get("task-stop-running").on_click({}, setup.cx);
  setup.nodes.get("tasks-more").on_click({}, setup.cx);
  assert.deepEqual(plain(setup.calls.slice(2)), [["open", "original-session"], ["cancel", {id:"queued"}], ["stop", {id:"running"}], ["refresh"]]);
  assert.equal(setup.view.pages, 2);
  assert.equal(setup.view.tab, 1);
});

test("a data refresh preserves the selected Runs tab", async () => {
  const main = (await readFile(new URL("../dev.sailry.platform/desktop/main.js", import.meta.url), "utf8"))
    .replace(/^import .*;\n/gm, "").replace("export default class", "class");
  const ScheduledTasks = vm.runInNewContext(`${main}\nScheduledTasks`, {
    View:class {}, context:() => JSON.stringify({locale:"en"}), messages,
    list:async() => ({Ok:{items:[{id:"fresh"}], after:null}}),
    history:async() => ({Ok:{items:[{job:{id:"run"}}], next_before:null}}),
    states:async() => ({fresh:"running"}),
    readProjectCatalog:async() => ({projects:[], worktrees:[]}),
    listModels:async() => ({models:[]}),
  });
  const view = new ScheduledTasks(), jobs = [], cx = {notify() {}, spawn:job => jobs.push(job)};
  view.init(null, cx);
  assert.equal(view.loaded, false);
  jobs.length = 0;
  view.tab = 1;
  view.refresh(cx);
  await jobs.shift()(cx);
  assert.equal(view.tab, 1);
  assert.equal(view.loading, false);
  assert.equal(view.loaded, true);
  assert.equal(view.items[0].id, "fresh");
  assert.equal(view.items[0].status, "running");
  assert.equal(view.history[0].job.id, "run");
  assert.equal("queues" in view, false);
});

test("task cards combine title and project before fixed status, toggle and actions", () => {
  const setup=fixture(), item={id:"row",name:"Review 中文 🙂",prompt:"Keep this task in the editor",revision:"2",project:"project",queue:"default",enabled:true,next_ms:null};
  setup.view.items=[item];setup.render();
  const row=setup.nodes.get("task-card-row");
  assert.equal(row.kind,"CardRow");
  assert.deepEqual(plain(row.props),{row_id:"task-row-row"});
  assert.deepEqual(row.items.map(child=>child.kind),["CardSummary","CardColumn","CardColumn","CardColumn"]);
  assert.deepEqual(row.items.map(child=>child.selector),["task-summary-row","task-status-row","task-leading-row","task-actions-row"]);
  assert.deepEqual(row.items.slice(1).map(child=>child.props.variant),["metadata","control","actions"]);
  assert.deepEqual(plain(row.items[0].props),{title:item.name,subtitle:"Project",icon:"icons/folder.svg"});
  assert.equal(setup.nodes.get("task-enabled-row").kind,"Toggle");
  assert.deepEqual(plain(setup.nodes.get("task-enabled-row").props),{label:"Enabled: Review 中文 🙂",checked:true,disabled:false});
  const actions=setup.nodes.get("task-actions-row");
  assert.deepEqual(plain(actions.props),{variant:"actions"});
  assert.deepEqual(actions.items.map(child=>child.selector),["task-run-row","task-edit-row","task-delete-row"]);
  assert.ok(actions.items.every(child=>child.kind==="IconButton"));
  assert.deepEqual(plain(actions.items.map(child=>child.props)),[
    {icon:"reicon:video/play",label:setup.view.text.run,disabled:false},
    {icon:"reicon:newicons/edit",label:setup.view.text.edit,disabled:false},
    {icon:"reicon:ui/trash2",label:setup.view.text.remove,disabled:false},
  ]);
  assert.equal(JSON.stringify(plain(row.items)).includes(item.prompt),false);
  setup.view.control({id:"task-edit-row"},setup.cx);
  assert.equal(setup.calls[0][1].prompt,item.prompt);
  setup.view.control({id:"task-delete-row"},setup.cx);
  assert.equal(setup.view.deleting,item);
  setup.render();
  setup.nodes.get("task-delete-confirm").on_click({},setup.cx);
  assert.deepEqual(plain(setup.calls[1]),["remove",{id:"row",revision:"2"}]);
});

test("task status follows native state independently of history and retains the next time", () => {
  const setup=fixture(), next=1792000000000;
  const item={id:"row",name:"Review",project:null,enabled:true,next_ms:next};
  setup.view.items=[item];
  setup.view.history=[{job:{handler:"row",status:"running"}}];
  const states=[
    [{...item,status:null},"Pending"],
    [{...item,status:"queued"},"Queued"],
    [{...item,status:"running",enabled:false},"Running"],
    [{...item,status:"completed"},"Pending"],
    [{...item,status:"completed",next_ms:null},"Completed"],
    [{...item,status:"failed",next_ms:null},"Failed"],
    [{...item,enabled:false},"Paused"],
  ];
  for(const [task,expected] of states) {
    setup.view.items=[task];setup.render();
    const tooltip=setup.nodes.get("task-schedule-row");
    assert.equal(tooltip.items[0].items[0],expected);
    assert.equal(tooltip.props.text,task.next_ms===null?expected:`${expected} · ${new Date(next).toLocaleString()}`);
  }
});

test("task pages do not expose core queue controls or settings", () => {
  const setup=fixture();
  setup.view.queues=[{name:"default",revision:"9007199254740993",concurrency:2,paused:false}];
  for(const tab of [0,1]) {
    setup.view.tab=tab;setup.render();
    assert.equal([...setup.nodes.keys()].some(id=>id.startsWith("task-queue-")||id.startsWith("queue-")),false);
    assert.equal(JSON.stringify(plain(setup.nodes.get(tab?"tasks-runs":"tasks-items"))).includes("Concurrency"),false);
  }
});

test("native row controls reject stale identities, invalid values and pending mutations", () => {
  const setup=fixture(),item={id:"row",name:"Review",enabled:true};setup.view.items=[item];
  setup.view.control({id:"task-enabled-row",value:"false"},setup.cx);
  setup.view.control({id:"task-run-removed"},setup.cx);
  for(const state of [{busy:true,pending:null},{busy:false,pending:"original-request"}]) {
    Object.assign(setup.view,state);setup.render();
    for(const action of ["enabled","run","edit","delete"]) {
      assert.equal(setup.nodes.get(`task-${action}-row`).props.disabled,true);
      setup.view.control({id:`task-${action}-row`,value:false},setup.cx);
    }
  }
  assert.deepEqual(setup.calls,[]);assert.equal(setup.view.deleting,null);
  assert.equal(setup.view.dialogId,0);
});

test("execution cards share content, status and action roles without changing outcomes", () => {
  const setup=fixture();setup.view.tab=1;
  setup.view.history=["completed","failed","unknown","queued","running"].map(status=>({
    name:`Review ${status}`,session:status==="failed"?"session-failed":null,turn:status==="running"?"turn":null,
    job:{id:status,created_ms:0,status,...(status==="failed"?{error:{message:"Provider unavailable"}}:{})},
  }));
  setup.render();
  for(const item of setup.view.history) {
    const {job}=item,row=setup.nodes.get(`task-execution-${job.id}`);
    assert.equal(row.kind,"CardRow");
    assert.ok(row.items.every(child=>child.kind==="CardColumn"));
    const hasActions=["failed","queued","running"].includes(job.status);
    assert.deepEqual(row.items.map(child=>child.props.variant),hasActions?["content","inline","actions"]:["content","inline"]);
    assert.deepEqual(row.items.map(child=>child.selector),[`task-execution-content-${job.id}`,`task-execution-status-${job.id}`,...(hasActions?[`task-execution-actions-${job.id}`]:[])]);
    assert.deepEqual(plain(row.items[0].items[0].items[0].items),[item.name]);
    const date=setup.nodes.get(`task-execution-time-${job.id}`);
    assert.equal(date.kind,"CardColumn");
    assert.deepEqual(plain(date.props),{variant:"inline"});
    assert.deepEqual(plain(date.items),[new Date(0).toLocaleString()]);
    const status=setup.nodes.get(`task-execution-status-${job.id}`);
    assert.equal(status.items[0].kind,"div");
    assert.deepEqual(plain(status.items[0].items),[setup.view.text[job.status==="failed"?"runFailed":job.status]]);
    assert.deepEqual(plain(status.items[0].style.text_color),[job.status==="completed"?"success":["failed","unknown"].includes(job.status)?"danger":"muted"]);
    assert.equal(setup.nodes.has(`task-execution-actions-${job.id}`),hasActions);
    if(hasActions)assert.deepEqual(plain(row.items[2].props),{variant:"actions",spacing:"regular"});
    assert.equal(setup.nodes.has(`task-stop-${job.id}`),["queued","running"].includes(job.status));
  }
  const detail=setup.nodes.get("task-error-detail-failed");
  assert.equal(detail.kind,"Tooltip");
  assert.deepEqual(plain(detail.props),{text:"Provider unavailable"});
  assert.equal(detail.items[0].selector,"task-details-failed");
  assert.deepEqual(setup.nodes.get("task-execution-actions-failed").items.map(child=>child.selector),["task-error-detail-failed","task-open-failed"]);
  setup.nodes.get("task-open-failed").on_click({},setup.cx);
  setup.nodes.get("task-stop-queued").on_click({},setup.cx);
  setup.nodes.get("task-stop-running").on_click({},setup.cx);
  assert.deepEqual(plain(setup.calls),[["open","session-failed"],["cancel",{id:"queued"}],["stop",{id:"running"}]]);
});

test("host scope retains public task capabilities and localized tab copy", async () => {
  const manifest = JSON.parse(await readFile(new URL("../plugin.json", import.meta.url), "utf8"));
  const extension = manifest.extensions["dev.sailry.platform"];
  assert.equal(extension.scope, "host");
  assert.equal(extension.api_version, "v1");
  assert.equal(manifest.version, "0.1.0");
  assert.deepEqual(extension.actions, ["storage.read", "storage.write", "dispatch.manage", "sessions.start", "projects.read", "models.read", "conversation.read"]);
  assert.equal(extension.tools.length,1);
  assert.deepEqual(extension.tools[0].contexts,["workspace","plugin"]);
  assert.deepEqual(extension.tools[0].handler.flow.operations,["storage.transaction"]);
  assert.equal(extension.host.turn.initialize,"initialize");
  for (const name of ["project","worktree","config","model"]) assert.equal(name in extension.tools[0].handler.parameters.properties,false);
  assert.equal(messages("zh-CN").tasks, "任务");
  assert.equal(messages("zh-CN").runs, "记录");
  assert.equal(messages("en").runs, "Records");
  assert.equal(extension.host.handlers.includes("queue"),false);
  assert.equal(messages("zh-CN").description, "按计划运行任务");
  assert.equal(messages("en").defaultModel, undefined);
  assert.equal(messages("en").modelPlaceholder, "Choose a model");
  assert.equal(messages("zh-CN").strength, "强度");
});

test("the editor uses the shared modal title, body, footer and close guard", () => {
  const setup=fixture();setup.view.editing={draft:{}};setup.view.busy=true;setup.render();
  const dialog=setup.nodes.get("task-dialog-0");
  assert.deepEqual(plain(dialog.props),{open:true,dismissable:false,form:true,title:"New task",width:560});
  assert.deepEqual(dialog.items.map(item=>item.kind),["editor","footer"]);
});
