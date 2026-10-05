import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import {messages} from '../dev.sailry.platform/desktop/locales.js';

const tick = () => new Promise(resolve => setImmediate(resolve));
function events() {
  let resolve;
  return {
    next:() => new Promise(done => { resolve = done; }),
    send(value) { const done = resolve; resolve = undefined; assert.ok(done); done(value); }
  };
}
async function settings(bindings) {
  const source = (await readFile(new URL('../dev.sailry.platform/desktop/settings.js',import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?from '[^']+';\n/gm,'').replace('export default class ','class ');
  const activation = events(), changes = events(), controls = events(),notices=[];
  const Type = vm.runInNewContext(`${source}\nSettings`,{
    View:class {},context:() => '{"locale":"en"}',messages,
    nextWindowActivation:activation.next,nextChange:changes.next,nextControlEvent:controls.next,toast:value=>notices.push(value),...bindings
  });
  const work = [], cx = {notify() {},spawn(action) { const task = action(cx); work.push(task); return task; }};
  const view = new Type(); view.init({},cx);
  return {view,cx,activation,changes,controls,notices,ready:() => work[0],async act(action) {
    const before = work.length; action(); await Promise.all(work.slice(before)); await tick();
  }};
}
function permissions(local = true) {
  return {node:'execution-node',platform:'macos',local,screen_capture:false,accessibility:false};
}

test('permission requests are explicit and activation rechecks actual OS grants', async () => {
  let value = permissions(), reads = 0, requests = [];
  const {view,cx,activation,ready,act} = await settings({
    readComputerPermissions:async () => { reads++; return {...value}; },
    requestComputerPermission:async permission => { requests.push(permission); return {...value}; }
  });
  await ready();
  activation.send({cursor:'1',active:true}); await tick();
  assert.equal(reads,1);
  assert.equal(requests.length,0);
  assert.equal(view.canRequest('screen_capture'),true);
  await act(() => view.read('screen_capture',cx));
  assert.deepEqual(requests,['screen_capture']);
  assert.equal(view.canRequest('screen_capture'),true);
  assert.equal(view.status('screen_capture'),'computer_permission_missing');
  activation.send({cursor:'2',active:false}); await tick();
  value = {...value,screen_capture:true,accessibility:true};
  activation.send({cursor:'3',active:true}); await tick();
  assert.equal(reads,2);
  assert.equal(view.canRequest('screen_capture'),false);
  assert.equal(view.status('screen_capture'),'computer_permission_granted');
  assert.deepEqual(requests,['screen_capture']);
});

test('remote and unsupported Nodes expose status without local permission actions', async () => {
  for (const value of [permissions(false),{...permissions(),platform:'linux',screen_capture:null,accessibility:null}]) {
    let requests = 0;
    const {view,cx,ready,act} = await settings({
      readComputerPermissions:async () => value,
      requestComputerPermission:async () => { requests++; return value; }
    });
    await ready();
    assert.equal(view.canRequest('screen_capture'),false);
    assert.equal(view.canRequest('accessibility'),false);
    await act(() => view.read('accessibility',cx));
    assert.equal(requests,0);
    assert.equal(view.status('screen_capture'),value.platform === 'macos'
      ? 'computer_permission_missing' : 'computer_permission_unknown');
  }
});

test('coalesces refreshes during an explicit request without repeating the request', async () => {
  let reads = 0, requests = 0, resolve;
  const {view,cx,ready,act} = await settings({
    readComputerPermissions:async () => { reads++; return permissions(); },
    requestComputerPermission:() => { requests++; return new Promise(done => { resolve = done; }); }
  });
  await ready();
  const pending = act(() => view.read('accessibility',cx));
  assert.equal(view.pending,true);
  view.read(null,cx); view.read(null,cx); view.read('accessibility',cx);
  resolve(permissions()); await pending;
  assert.equal(view.pending,false);
  assert.equal(reads,2);
  assert.equal(requests,1);
});

test('SDK node-validation failures recover through refresh and reconnection', async () => {
  let fail = true, reads = 0;
  const {view,ready,controls,changes,notices} = await settings({
    readComputerPermissions:async () => {
      reads++;
      if (fail) throw new Error('computer permission response belongs to another Node');
      return permissions();
    }
  });
  await ready();
  assert.equal(view.status('screen_capture'),'computer_permission_unknown');
  assert.equal(notices.length,1);assert.equal(notices[0].kind,'error');
  assert.equal(view.canRequest('screen_capture'),false);
  fail = false;
  controls.send({id:'computer-permissions-refresh'}); await tick();
  assert.equal(view.canRequest('screen_capture'),true);
  changes.send({cursor:'1',connected:false}); await tick();
  assert.equal(view.value(),null);
  assert.equal(view.status('screen_capture'),'plugins_disconnected');
  controls.send({id:'computer-permissions-refresh'}); await tick();
  assert.equal(reads,2);
  changes.send({cursor:'2',connected:true}); await tick();
  assert.equal(reads,3);
  assert.equal(view.canRequest('screen_capture'),true);
});

test('a failed refresh keeps previous permission data and reports only through a toast',async()=>{
  let fail=false;
  const setup=await settings({readComputerPermissions:async()=>{if(fail)throw new Error('Unavailable');return permissions();}});
  await setup.ready();const previous=setup.view.value();fail=true;await setup.act(()=>setup.view.read(null,setup.cx));
  assert.equal(setup.view.value(),previous);assert.equal(setup.view.status('screen_capture'),'computer_permission_missing');
  assert.equal(setup.view.canRequest('screen_capture'),false);assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].kind,'error');
});

test('closing permission guidance preserves status without feedback or another request',async()=>{
  let requests=0;
  const setup=await settings({readComputerPermissions:async()=>permissions(),requestComputerPermission:async()=>{requests++;return null;}});
  await setup.ready();const previous=setup.view.value();await setup.act(()=>setup.view.read('screen_capture',setup.cx));
  assert.equal(setup.view.value(),previous);assert.equal(setup.view.pending,false);assert.equal(setup.view.error,null);
  assert.equal(setup.view.status('screen_capture'),'computer_permission_missing');assert.equal(setup.view.canRequest('screen_capture'),true);
  assert.equal(setup.notices.length,0);assert.equal(requests,1);
});
