import test from "node:test";
import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";
import {fixture,id,plain} from "./store.mjs";

test("initialization captures the execution scope and explicit session config",async()=>{
  const setup=await fixture(), initialized=await setup.initialize();
  assert.deepEqual(setup.turn(),{project:id(1),worktree:id(2),config:setup.config});
  assert.match(initialized.instruction,/Current UTC: .* ms/);
  setup.session.config={...setup.config,model:"another"};
  assert.equal(setup.turn().config.model,"chosen");
  assert.deepEqual(plain((await setup.initialize({...setup.config,assistant:{package:"assistant"}})).tools),[]);
});

test("creation uses one admitted storage and dispatch transaction",async()=>{
  const setup=await fixture(), timing={kind:"every",data:{anchor_ms:Date.now()+3_600_000,interval_ms:86_400_000}};
  const reply=await setup.plan({action:"create",name:"Daily brief",prompt:"Prepare a brief 中文 🙂",timing});
  assert.equal(reply.call.operation,"storage.transaction");assert.equal(reply.result,undefined);
  assert.equal(setup.values.size,0);assert.equal(setup.transactions.length,0);
  assert.deepEqual(plain(reply.call.arguments.operations).map(operation=>operation.kind==="dispatch"?operation.data.kind:operation.kind),
    ["write","save_schedule","save_handler"]);
  const callback=reply.call.arguments.operations.find(operation=>operation.data.kind==="save_handler").data.data.callback;
  assert.equal(callback.command.kind,"start_session");assert.equal(callback.completion,"turn");
  assert.equal(callback.command.data.project,id(1));assert.equal(callback.command.data.worktree,id(2));
  assert.equal(reply.call.arguments.operations.find(operation=>operation.data.kind==="save_handler").data.data.queue,"default");
  assert.ok(setup.requests.every(request=>request.data?.action?.kind!=="list_queues"));
  assert.deepEqual(plain(callback.command.data.config),setup.config);
  const {result}=await setup.commit(reply);
  assert.equal(result.id,id(10));assert.equal(result.revision,"1");assert.equal(result.enabled,true);
  const listed=await setup.plan({action:"list"});
  assert.equal(listed.result.items.length,1);assert.deepEqual(plain(listed.result.items[0].timing),timing);
  assert.equal("queues" in listed.result,false);
});

test("captured display uses shared summary and localized action-target approval templates",async()=>{
  const manifest=JSON.parse(await readFile(new URL("../plugin.json",import.meta.url),"utf8"));
  const tool=manifest.extensions["dev.sailry.platform"].tools[0];
  assert.equal(tool.presentation,"summary");assert.deepEqual(tool.display.input,{summary:{path:"/name"}});
  assert.equal(tool.display.output,undefined);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.message.label),[
    "Create scheduled task %{name}","Update scheduled task %{name}","Update scheduled task %{id}","Delete scheduled task %{id}",
  ]);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.message.locales["zh-CN"]),[
    "创建定时任务 %{name}","更新定时任务 %{name}","更新定时任务 %{id}","删除定时任务 %{id}",
  ]);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.values),[
    {name:{source:"arguments",path:"/name"}},{name:{source:"arguments",path:"/name"}},
    {id:{source:"arguments",path:"/id"}},{id:{source:"arguments",path:"/id"}},
  ]);
  assert.deepEqual(tool.display.approval.map(prompt=>prompt.when.all[0]),["create","update","update","delete"]
    .map(action=>({value:{source:"arguments",path:"/action"},equals:action})));
  assert.deepEqual(tool.display.approval[1].when.all[1],{value:{source:"arguments",path:"/name"},equals:null,negate:true});
  assert.deepEqual(tool.display.approval[2].when.all[1],{value:{source:"arguments",path:"/name"},equals:null});
});

test("updates preserve saved model and scope and deletion cancels the matching schedule",async()=>{
  const setup=await fixture(), item=setup.task(11);
  item.config.model="saved-model";item.queue="custom";item.timing={kind:"every",data:{anchor_ms:1,interval_ms:86_400_000}};
  setup.seed(item);
  const reply=await setup.plan({action:"update",id:item.id,revision:"1",name:"Reviewed",enabled:true});
  const saved=(await setup.commit(reply)).result;
  assert.equal(saved.revision,"2");assert.equal(saved.name,"Reviewed");assert.equal(saved.config.model,"saved-model");
  assert.equal(saved.project,item.project);assert.equal(saved.worktree,item.worktree);assert.equal(saved.queue,"custom");
  assert.deepEqual(plain(saved.timing),item.timing);assert.equal(saved.prompt,item.prompt);
  const removal=await setup.plan({action:"delete",id:item.id,revision:"2"});
  const handler=removal.call.arguments.operations.find(operation=>operation.data.kind==="remove_handler");
  assert.equal(handler.data.data.cancel_pending,true);
  const deleted=(await setup.commit(removal)).result;
  assert.deepEqual(plain(deleted),{id:item.id,revision:"3",removed:true});
  assert.equal(setup.values.get(`task/${item.id}`).present,false);assert.equal(setup.schedules.has(item.id),false);assert.equal(setup.handlers.has(item.id),false);
});

test("project and unassigned tools do not list or mutate another scope",async()=>{
  const setup=await fixture();
  const current=setup.task(11), foreign=setup.task(12,id(4)), global=setup.task(13,null);
  for(const item of [current,foreign,global])setup.seed(item);
  assert.deepEqual(plain((await setup.plan({action:"list"})).result.items).map(item=>item.id),[current.id]);
  for(const item of [foreign,global]) {
    for(const args of [{action:"update",id:item.id,revision:"1",enabled:true},{action:"delete",id:item.id,revision:"1"}]) {
      const reply=await setup.plan(args);assert.equal(reply.result.error.code,"permission_denied");assert.equal(reply.call,undefined);
    }
  }
  setup.project=null;await setup.initialize();
  assert.equal(setup.turn().worktree,null);
  assert.deepEqual(plain((await setup.plan({action:"list"})).result.items).map(item=>item.id),[global.id]);
  const reply=await setup.plan({action:"create",name:"Global",prompt:"Review",enabled:false,timing:{kind:"once",data:{at_ms:Date.now()+3_600_000}}});
  const saved=(await setup.commit(reply)).result;
  assert.equal(saved.project,null);assert.equal(saved.worktree,null);
});

test("strict arguments and stale revisions fail before a mutation is prepared",async()=>{
  const setup=await fixture(), item=setup.task(11);setup.seed(item);
  const create={action:"create",name:"Review",prompt:"Read",timing:{kind:"once",data:{at_ms:Date.now()+3_600_000}}};
  for(const args of [
    {...create,project:id(4)},{...create,config:setup.config},{...create,model:"another"},{...create,enabled:null},
    {...create,timing:{kind:"once",data:{at_ms:Date.now()-1}}},
    {...create,timing:{kind:"once",data:{at_ms:Date.now()+3_600_000,timezone:"UTC"}}},
    {...create,timing:{kind:"every",data:{anchor_ms:Date.now()+3_600_000,interval_ms:0}}},
    {action:"update",id:item.id,revision:"1"},{action:"delete",id:item.id,revision:"0",prompt:"Unexpected"},
  ]) {
    const reply=await setup.plan(args);assert.equal(reply.result.isError,true);assert.equal(reply.result.error.code,"invalid_request");assert.equal(reply.call,undefined);
  }
  const stale=await setup.plan({action:"update",id:item.id,revision:"2",prompt:"Changed"});
  assert.equal(stale.result.error.code,"revision_conflict");assert.equal(stale.call,undefined);
  assert.equal(setup.values.size,1);assert.equal(setup.schedules.size,0);assert.equal(setup.transactions.length,0);
});

test("continuations preserve uncertainty and never claim success without a receipt",async()=>{
  const setup=await fixture(), reply=await setup.plan({action:"create",name:"Review",prompt:"Read",timing:{kind:"once",data:{at_ms:Date.now()+3_600_000}}});
  const unknown={isError:true,error:{code:"outcome_unknown",message:"Result unavailable"}};
  assert.equal((await setup.finish(reply,unknown)).result,unknown);
  const missing=await setup.finish(reply,{kind:"plugin_transaction",data:[]});
  assert.equal(missing.result.error.code,"outcome_unknown");assert.equal(setup.values.size,0);
});

test("filtered paging advances through foreign records without skipping owned tasks",async()=>{
  const setup=await fixture();
  for(let index=11;index<44;index++)setup.seed(setup.task(index,id(4)));
  setup.seed(setup.task(44));
  const first=(await setup.plan({action:"list"})).result;
  assert.equal(first.items.length,0);assert.equal(first.after,`task/${id(42)}`);
  const second=(await setup.plan({action:"list",after:first.after})).result;
  assert.deepEqual(plain(second.items).map(item=>item.id),[id(44)]);assert.equal(second.after,null);
});
