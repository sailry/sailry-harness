import test from "node:test";
import assert from "node:assert/strict";
import {fixture, item, id, project, foreign, clone} from "./store.mjs";

const start = setup => setup.main.initialize({project,tools:["reminders"]});
const call = (setup, args) => setup.main.reminders({arguments:args,step:0,state:null,outcome:null});
const finish = (setup, transition, outcome) => setup.main.reminders({arguments:{},step:1,state:transition.state,outcome});

test("requires explicit canonical project or null in drafts and stored records", async () => {
  const setup = await fixture();
  assert.equal(setup.records.draft(item(1,{project:null})).project,null);
  assert.equal(setup.records.draft(item(1)).project,project);
  for (const value of [undefined,"", "other"]) assert.throws(() => setup.records.draft(item(1,{project:value})),/Invalid reminder ID/);
  const previous = item(1); delete previous.project; setup.seed(previous);
  await assert.rejects(setup.records.read(id(1)),/storage is incompatible/);
  await assert.rejects(setup.records.page(),/storage is incompatible/);
  const saved = await setup.main.save({...item(1),revision:"1"});
  assert.equal(saved.Err.message,"Reminder storage is incompatible");
  assert.deepEqual(setup.get(`reminder/${id(1)}`).value,previous);
  assert.equal(setup.prepared.length,0);
});

test("desktop creates unassigned reminders and explicitly validates chosen project", async () => {
  const setup = await fixture();
  const unassigned = await setup.main.save({...item(1,{project:null}),revision:"0"});
  assert.equal(unassigned.Ok.project,null);
  assert.equal(setup.catalogReads.length,0);
  const associated = await setup.main.save({...item(2),revision:"0"});
  assert.equal(associated.Ok.project,project);
  assert.equal(setup.catalogReads.length,1);
  const rejected = await setup.main.save({...item(3,{project:id(900)}),revision:"0"});
  assert.equal(rejected.Err.code,"not_found");
  assert.equal(setup.get(`reminder/${id(3)}`).present,false);
  assert.equal(setup.prepared.length,2);
});

test("missing project associations remain intact until management explicitly changes them", async () => {
  const setup = await fixture(); setup.seed(item(1)); setup.catalog={projects:[],worktrees:[]};
  assert.equal((await setup.main.list({})).Ok.items[0].project,project);
  const rejected = await setup.main.save({...item(1,{title:"Changed"}),revision:"1"});
  assert.equal(rejected.Err.code,"not_found");
  assert.equal(setup.get(`reminder/${id(1)}`).value.project,project);
  assert.equal(setup.prepared.length,0);
  const changed = await setup.main.save({...item(1,{project:null}),revision:"1"});
  assert.equal(changed.Ok.project,null);
});

test("captures the execution project without contributing reminder data to model instructions", async () => {
  const setup = await fixture();
  assert.deepEqual(clone(await start(setup)),{instruction:"",tools:["reminders"]});
  assert.deepEqual(setup.state,{project});
  assert.deepEqual(clone(await setup.main.initialize({project:null,tools:["reminders"]})),{instruction:"",tools:[]});
  assert.equal((await call(setup,{action:"list"})).result.error.code,"permission_denied");
});

test("lists only current project reminders with bounded matching pagination", async () => {
  const setup = await fixture(); await start(setup);
  for (let index=1;index<=170;index++) setup.seed(item(index,{project:index<=140 ? foreign : index%2 ? project : null}));
  const first = (await call(setup,{action:"list"})).result;
  assert.equal(first.items.length,0); assert.equal(first.after,`reminder/${id(128)}`);
  assert.equal(setup.scans.length,4);
  const second = (await call(setup,{action:"list",after:first.after})).result;
  assert.equal(second.items.length,15); assert.equal(second.after,null);
  assert.ok(second.items.every(value => value.project === project && value.revision === "1"));
  assert.ok(setup.scans.every(value => value.limit===32));
  assert.equal((await call(setup,{action:"list",after:"private/elsewhere"})).result.error.code,"invalid_request");
});

test("list pages contain at most 32 matching entries and honor the continuation", async () => {
  const setup = await fixture(); await start(setup);
  for (let index=1;index<=45;index++) setup.seed(item(index));
  const first = (await call(setup,{action:"list"})).result;
  const second = (await call(setup,{action:"list",after:first.after})).result;
  assert.equal(first.items.length,32); assert.equal(second.items.length,13); assert.equal(second.after,null);
  assert.equal(new Set([...first.items,...second.items].map(value => value.id)).size,45);
});

test("create uses a stable canonical call ID and one atomic scheduled transaction", async () => {
  const setup = await fixture(); await start(setup);
  const args = {action:"create",title:"Follow up",message:"Check result",due_ms:Date.now()+300000};
  const first = await call(setup,args), repeated = await call(setup,args);
  assert.deepEqual(clone(first),clone(repeated));
  assert.equal(first.call.operation,"storage.transaction");
  const operations = first.call.arguments.operations;
  assert.equal(operations[0].data.value.id,id(1000)); assert.equal(operations[0].data.value.project,project);
  assert.equal(operations[0].data.expected_revision,"0");
  assert.deepEqual(Array.from(operations.slice(1),value => value.data.kind),["save_schedule","save_handler"]);
  assert.equal(operations[1].data.data.payload.revision,"1");
  assert.equal(setup.prepared.length,0); assert.equal(setup.catalogReads.length,0);
  const outcome = setup.apply(operations), result = await finish(setup,first,outcome);
  assert.equal(result.result.id,id(1000)); assert.equal(result.result.revision,"1");
  assert.equal(setup.schedules.get(id(1000)).enabled,true);
  assert.equal((await call(setup,args)).result.error.code,"revision_conflict");
  assert.equal(setup.entries.size,1);
});

test("update and delete require revisions and reject foreign or unassigned records before schedule reads", async () => {
  const setup = await fixture(); await start(setup);
  setup.seed(item(1,{project:foreign})); setup.seed(item(2,{project:null})); setup.seed(item(3));
  for (const action of ["update","delete"]) for (const number of [1,2]) {
    const result = await call(setup,{action,id:id(number),revision:"1",...(action==="update" ? {title:"Changed"} : {})});
    assert.equal(result.result.error.code,"permission_denied");
  }
  for (const args of [{action:"update",id:id(3),title:"Changed"},{action:"delete",id:id(3)},
    {action:"update",id:id(3),revision:1,title:"Changed"},{action:"update",id:id(3),revision:"1"},
    {action:"update",id:id(3),revision:"2",title:"Changed"}]) assert.ok((await call(setup,args)).result.isError);
  assert.equal(setup.queries.length,0); assert.equal(setup.prepared.length,0);
});

test("updates editable fields and returns actual revisions before deleting the same project reminder", async () => {
  const setup = await fixture(); await start(setup); setup.seed(item(1,{message:"Keep note"}),"4");
  const changed = await call(setup,{action:"update",id:id(1),revision:"4",completed:true});
  const value = changed.call.arguments.operations[0].data.value;
  assert.equal(value.message,"Keep note"); assert.equal(value.project,project); assert.equal(value.completed,true);
  const result = await finish(setup,changed,setup.apply(changed.call.arguments.operations));
  assert.equal(result.result.revision,"5");
  const removed = await call(setup,{action:"delete",id:id(1),revision:"5"});
  assert.deepEqual(Array.from(removed.call.arguments.operations,value => value.kind),["remove","dispatch","dispatch"]);
  assert.deepEqual(clone((await finish(setup,removed,setup.apply(removed.call.arguments.operations))).result),
    {id:id(1),revision:"6",removed:true});
  assert.equal(setup.get(`reminder/${id(1)}`).present,false);
  assert.equal(setup.schedules.size,0); assert.equal(setup.handlers.size,0);
});

test("strict actions reject model supplied project, ID on create and unrelated fields", async () => {
  const setup = await fixture(); await start(setup);
  for (const args of [{action:"create",title:"New",id:id(1)}, {action:"create",title:"New",project:foreign},
    {action:"create",title:"New",completed:null},{action:"list",title:"Ignore"},
    {action:"delete",id:id(1),revision:"1",due_ms:null},{action:"create",title:"New",message:null}]) {
    assert.equal((await call(setup,args)).result.error.code,"invalid_request");
  }
  assert.equal(setup.queries.length,0); assert.equal(setup.prepared.length,0);
});

test("failed and uncertain continuations return the outcome without another call or replay", async () => {
  const setup = await fixture(); await start(setup);
  const transition = await call(setup,{action:"create",title:"New"}), count = setup.queries.length;
  for (const code of ["revision_conflict","outcome_unknown","cancelled"]) {
    const outcome = {isError:true,error:{code,message:"Unconfirmed"}};
    assert.deepEqual(clone(await finish(setup,transition,outcome)),{result:outcome});
  }
  const missing = await finish(setup,transition,{kind:"plugin_transaction",data:[]});
  assert.equal(missing.result.error.code,"outcome_unknown");
  assert.equal(setup.queries.length,count); assert.equal(setup.prepared.length,0); assert.equal(setup.entries.size,0);
});

test("dispatch conflicts leave reminder and schedule changes atomic", async () => {
  const setup = await fixture(); await start(setup); setup.seed(item(1));
  const transition = await call(setup,{action:"update",id:id(1),revision:"1",title:"Changed"});
  setup.schedules.set(id(1),{id:id(1),revision:1});
  assert.throws(() => setup.apply(transition.call.arguments.operations),/Dispatch changed/);
  assert.equal(setup.get(`reminder/${id(1)}`).value.title,"Review");
  assert.equal(setup.get(`reminder/${id(1)}`).revision,"1");
  assert.equal(setup.schedules.size,1); assert.equal(setup.schedules.get(id(1)).revision,1);
  assert.equal(setup.handlers.size,0);
});
