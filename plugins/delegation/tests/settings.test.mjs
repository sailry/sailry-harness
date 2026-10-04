import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import * as draft from '../dev.sailry.platform/desktop/draft.js';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/settings.js',import.meta.url),'utf8'))
  .replace(/^import[\s\S]*?from '[^']+';\n/gm,'').replace('export default class ','class ');
const plain = value => JSON.parse(JSON.stringify(value));
const tick = () => new Promise(resolve => setImmediate(resolve));
const deferred = () => { let finish; return {promise:new Promise(resolve => {finish = resolve;}),finish:value => finish(value)}; };
function events() {
  let resolve;
  return {next:() => new Promise(done => {resolve = done;}),send(value) {
    assert.ok(resolve); const done = resolve; resolve = null; done(value);
  }};
}
const model = {kind:'provider',id:'provider/model',provider:'Provider',model:'model',reasoning:true,
  default:true,efforts:['low','high'],default_effort:'high'};
const role = {id:'existing',revision:5,key:'review',name:'Review',appearance:null,description:'Kept',
  model:null,max_turns:null,skills:['retained'],instructions:'Original'};

async function settings(options = {}) {
  const changes = events(), controls = events(), menus = events(), closes = events(), inputs = events();
  const prepared = [], calls = [], forgotten = [], notices = [], released = [], fields = new Map();
  let profiles = [plain(role)], models = [plain(model)], ids = 0;
  const Type = vm.runInNewContext(`${source}\nSettings`,{
    View:class {},context:() => '{"locale":"en"}',draft,messages,
    nextChange:changes.next,nextControlEvent:controls.next,nextMenuEvent:menus.next,
    modal_closed:closes.next,nextTextEvent:inputs.next,
    createText:value => {const id = `field-${++ids}`; fields.set(id,value); return id;},
    readText:id => fields.get(id),setText:(id,value) => fields.set(id,value),
    releaseText:id => {released.push(id); fields.delete(id);},
    toast:value => notices.push(value),dismissToast() {},forgetRequest:id => forgotten.push(id),
    listRoles:async () => plain(profiles),listModels:async () => ({models:plain(models)}),newRoleId:() => 'created',
    prepareRole:(role,revision) => {prepared.push(plain({kind:'edit',role,revision})); return `request-${prepared.length}`;},
    prepareRemoveRole:(id,revision) => {prepared.push({kind:'remove',id,revision}); return `request-${prepared.length}`;},
    completeRequest:async id => {calls.push(id); const value = prepared[Number(id.split('-')[1])-1];
      return options.complete ? options.complete(id,value) : {Ok:{kind:value.kind === 'edit' ? 'role' : 'roles',data:{}}};},
    faultCode:() => 'invalid_request'
  });
  const cx = {notify() {},spawn(action) {return action(cx);}};
  const view = new Type(); view.init({},cx); await tick();
  return {view,cx,changes,controls,menus,closes,inputs,prepared,calls,forgotten,notices,released,fields,
    async act(action) {action(); await tick(); await tick();},
    catalog(roles,nextModels) {profiles = roles; if (nextModels) models = nextModels;},
    edit(field,value) {fields.set(view.dialog.fields[field],value);},
    choose(menu,value) {menus.send({menu,id:`${view.dialog.token}:${value}`});}
  };
}

test('saves the complete captured draft without changing hidden fields or model choices',async () => {
  const f = await settings(); f.view.edit(f.view.roles[0],f.cx);
  const owner = f.view.dialog;
  f.edit('key',' reviewer '); f.edit('name',' New name '); f.edit('turns','7');
  await f.act(() => f.choose('role-source','fixed'));
  f.catalog([{...role,revision:99}],[]);
  await f.act(() => f.changes.send({cursor:'1',connected:true}));
  await f.act(() => f.changes.send({cursor:'2',connected:true}));
  assert.equal(owner.models.length,1); assert.equal(owner.original.revision,5);
  await f.act(() => f.view.save(f.cx));
  assert.deepEqual(f.prepared,[{kind:'edit',revision:5,role:{...role,key:'reviewer',name:'New name',
    appearance:{icon:'ai',color:'none'},max_turns:7,model:{provider:'provider',model:'model',effort:'high'}}}]);
  assert.equal(f.view.dialog,null); assert.equal(f.released.length,4);
});

test('uncertain saves lock the draft and retry exactly the original receipt',async () => {
  let count = 0;
  const f = await settings({complete:async () => ++count === 1 ? {Err:{code:'outcome_unknown'}} : {Ok:{kind:'role',data:{}}}});
  f.view.edit(role,f.cx);
  await f.act(() => f.view.save(f.cx));
  const owner = f.view.dialog; assert.equal(owner.error,'role_unknown');
  await f.act(() => f.choose('role-source','fixed'));
  await f.act(() => f.controls.send({id:'role',value:{icon:'code',color:'red'}}));
  assert.equal(owner.model,null); assert.deepEqual(plain(owner.appearance),{icon:'ai',color:'none'});
  await f.act(() => f.view.save(f.cx));
  assert.deepEqual(f.calls,['request-1','request-1']); assert.equal(f.prepared.length,1);
  assert.deepEqual(f.forgotten,['request-1']); assert.equal(f.view.dialog,null);
});

test('revision conflicts preserve every edit and never automatically resubmit',async () => {
  const f = await settings({complete:async () => ({Err:{code:'revision_conflict'}})});
  f.view.edit(role,f.cx); f.edit('name','Retained draft'); f.edit('instructions','Changed instructions');
  await f.act(() => f.view.save(f.cx));
  assert.equal(f.view.dialog.error,'role_conflict'); assert.equal(f.view.dialog.request,null);
  assert.equal(f.fields.get(f.view.dialog.fields.name),'Retained draft');
  assert.equal(f.fields.get(f.view.dialog.fields.instructions),'Changed instructions');
  assert.equal(f.view.dialog.original.revision,5); assert.deepEqual(f.calls,['request-1']);
  assert.deepEqual(f.forgotten,['request-1']);
});

test('a closed admitted dialog cannot close or edit a newer dialog',async () => {
  const completion = deferred(), f = await settings({complete:() => completion.promise});
  f.view.edit(role,f.cx); const previous = f.view.dialog;
  await f.act(() => f.view.save(f.cx));
  f.view.close(f.cx); f.view.edit(null,f.cx); const current = f.view.dialog;
  await f.act(() => f.menus.send({menu:'role-source',id:`${previous.token}:fixed`}));
  await f.act(() => f.closes.send(previous.token));
  assert.equal(f.view.dialog,current); assert.equal(current.fixed,false);
  await f.act(() => completion.finish({Ok:{kind:'role',data:{}}}));
  assert.equal(f.view.dialog,current); assert.equal(current.closed,false);
});

test('presets replace only instructions and removal retries the captured role revision',async () => {
  let count = 0;
  const f = await settings({complete:async () => ++count === 1 ? {Err:{code:'unavailable'}} : {Ok:{kind:'roles',data:[]}}});
  f.view.edit(role,f.cx);
  await f.act(() => f.choose('role-presets','research'));
  assert.equal(f.fields.get(f.view.dialog.fields.instructions),messages('en').role_prompt_research);
  assert.equal(f.fields.get(f.view.dialog.fields.name),'Review');
  f.view.remove(role,f.cx);
  await f.act(() => f.view.save(f.cx));
  assert.equal(f.view.dialog.error,'role_unknown');
  await f.act(() => f.view.save(f.cx));
  assert.deepEqual(f.prepared,[{kind:'remove',id:'existing',revision:5}]);
  assert.deepEqual(f.calls,['request-1','request-1']); assert.equal(f.view.dialog,null);
});

test('disconnection blocks new actions while retaining an unsaved dialog',async () => {
  const f = await settings(); f.view.edit(role,f.cx); const owner = f.view.dialog;
  f.edit('name','Draft');
  await f.act(() => f.changes.send({cursor:'1',connected:false}));
  f.view.save(f.cx); f.view.remove(role,f.cx); f.view.edit(null,f.cx);
  assert.equal(f.view.dialog,owner); assert.equal(f.calls.length,0);
  assert.equal(f.fields.get(owner.fields.name),'Draft');
});
