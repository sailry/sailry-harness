import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/settings.js',import.meta.url),'utf8'))
  .replace(/^import[\s\S]*?from '[^']+';\n/gm,'').replace('export default class ','class ');
const tick = () => new Promise(resolve => setImmediate(resolve));
const plain = value => JSON.parse(JSON.stringify(value));
function events() {
  let resolve;
  return {next:() => new Promise(done => {resolve = done;}),send(value) {
    assert.ok(resolve); const done = resolve; resolve = null; done(value);
  }};
}
const initialModels = [
  {provider:'provider-a',provider_name:'First',model:'vision',kinds:['vision']},
  {provider:'provider-b',provider_name:'Second',model:'image',kinds:['image','video']}
];

async function settings(options = {}) {
  const changes = events(), menus = events(), work = [], prepared = [], calls = [], notices = [], forgotten = [];
  let saved = structuredClone(options.saved ?? {revision:0,bindings:{}}), models = structuredClone(initialModels);
  const Type = vm.runInNewContext(`${source}\nSettings`,{
    View:class {},context:() => '{"locale":"en"}',messages,
    nextChange:changes.next,nextControlEvent:menus.next,
    toast:value => notices.push(value),dismissToast() {},forgetRequest:id => forgotten.push(id),
    readMediaSettings:async () => structuredClone(saved),listMediaModels:async () => structuredClone(models),
    prepareMediaSettings:value => { prepared.push(plain(value)); return `request-${prepared.length}`; },
    completeRequest:async id => {
      calls.push(id);
      if (options.complete) return options.complete(id,prepared.at(-1));
      saved = {...prepared.at(-1),revision:prepared.at(-1).revision + 1};
      return {Ok:{kind:'media_settings',data:structuredClone(saved)}};
    },...options.bindings
  });
  const cx = {notify() {},spawn(action) { const task = action(cx); work.push(task); return task; }};
  const view = new Type(); view.init({},cx); await work[0];
  return {view,cx,prepared,calls,notices,forgotten,changes,menus,
    setSaved(value) {saved = structuredClone(value);},setModels(value) {models = structuredClone(value);},
    async act(action) { action(); for (let index = 0; index < 4; index++) await tick(); }
  };
}

test('selects and clears a role while retaining other configured roles', async () => {
  const fixture = await settings({saved:{revision:4,bindings:{image:{provider:'provider-b',model:'image'}}}});
  const {view,cx,menus,prepared} = fixture;
  const choices = view.items('vision-menu','vision');
  assert.equal(choices.length,2);
  assert.equal(choices[0].checked,true);
  assert.equal(choices[1].label,'First / vision');
  assert.equal(view.items('video-menu','video')[1].label,'Second / image');
  await fixture.act(() => menus.send({id:'vision-menu',value:choices[1].id}));
  assert.deepEqual(prepared[0],{revision:4,bindings:{
    vision:{provider:'provider-a',model:'vision'},image:{provider:'provider-b',model:'image'}
  }});
  assert.equal(view.saved.revision,5);
  assert.equal(view.label('vision'),'First / vision');
  assert.equal(view.items('vision-menu','vision')[1].checked,true);
  await fixture.act(() => view.select('vision-menu',view.items('vision-menu','vision')[0].id,cx));
  assert.deepEqual(plain(view.saved),{revision:6,bindings:{image:{provider:'provider-b',model:'image'}}});
  assert.equal(view.label(null),'Unavailable');
});

test('open menu choices retain their binding and reject stale revisions', async () => {
  const fixture = await settings(), {view,cx,prepared} = fixture;
  const captured = view.items('vision-menu','vision')[1];
  fixture.setModels([{provider:'new',provider_name:'New',model:'replacement',kinds:['vision']}]);
  await fixture.act(() => view.load(cx));
  view.items('vision-menu','vision');
  await fixture.act(() => view.select('vision-menu',captured.id,cx));
  assert.deepEqual(prepared[0].bindings.vision,{provider:'provider-a',model:'vision'});
  const stale = view.items('vision-menu','vision')[0];
  fixture.setSaved({revision:9,bindings:{}});
  await fixture.act(() => view.load(cx));
  await fixture.act(() => view.select('vision-menu',stale.id,cx));
  assert.equal(view.error,'media_conflict');
  assert.equal(prepared.length,1);
  assert.equal(view.saved.revision,9);
});

test('late reads cannot replace newer settings and missing providers remain explicit', async () => {
  const fixture = await settings({saved:{revision:7,bindings:{vision:{provider:'missing',model:'kept'}}}});
  const {view,cx} = fixture;
  assert.equal(view.label('vision'),'Model unavailable / kept');
  fixture.setSaved({revision:2,bindings:{}});
  await fixture.act(() => view.load(cx));
  assert.equal(view.saved.revision,7);
  assert.equal(view.label('vision'),'Model unavailable / kept');
});

test('disconnect and pending saves disable actions without duplicate dispatch', async () => {
  let finish;
  const fixture = await settings({complete:() => new Promise(resolve => {finish = resolve;})});
  const {view,cx,changes,calls} = fixture;
  const selected = view.items('vision-menu','vision')[1];
  changes.send({cursor:'1',connected:false}); await tick();
  view.select('vision-menu',selected.id,cx);
  assert.equal(calls.length,0);
  changes.send({cursor:'2',connected:true}); await tick();
  view.select('vision-menu',selected.id,cx);
  assert.equal(view.pending,true);
  view.select('vision-menu',selected.id,cx);
  assert.equal(calls.length,1);
  finish({Err:{code:'revision_conflict'}});
  await fixture.act(() => {});
  assert.equal(view.pending,false);
  assert.equal(view.error,'media_conflict');
  assert.equal(calls.length,1);
});

test('uncertain saves reconcile by reading and never replay the request', async () => {
  for (const completion of [
    async () => {throw new Error('receipt lost');},
    async () => ({Err:{code:'outcome_unknown'}})
  ]) {
    const fixture = await settings({complete:completion}), {view,cx,calls,forgotten} = fixture;
    fixture.setSaved({revision:1,bindings:{vision:{provider:'provider-a',model:'vision'}}});
    await fixture.act(() => view.select('vision-menu',view.items('vision-menu','vision')[1].id,cx));
    assert.equal(view.error,'media_unknown');
    assert.equal(view.saved.revision,1);
    assert.equal(view.label('vision'),'First / vision');
    assert.deepEqual(calls,['request-1']);
    assert.deepEqual(forgotten,['request-1']);
  }
});

test('business failures keep their existing localized feedback', async () => {
  for (const [code,key] of [['revision_conflict','media_conflict'],['invalid_request','media_model_unavailable'],
    ['not_configured','media_model_unavailable'],['unavailable','media_unknown'],['permission_denied','media_failed']]) {
    const fixture = await settings({complete:async () => ({Err:{code}})}), {view,cx,notices} = fixture;
    await fixture.act(() => view.select('vision-menu',view.items('vision-menu','vision')[1].id,cx));
    assert.equal(view.error,key);
    assert.equal(notices.at(-1).message,messages('en')[key]);
    assert.equal(notices.at(-1).kind,'error');
  }
});
