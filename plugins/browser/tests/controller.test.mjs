import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
import {messages,errorKey,imported} from '../dev.sailry.platform/desktop/locales.js';
import {destination} from '../dev.sailry.platform/desktop/address.js';

async function controller(file, bindings) {
  const source = (await readFile(new URL(`../dev.sailry.platform/desktop/${file}.js`,import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?from '[^']+';\n/gm,'')
    .replace('export default class ','class ');
  const name = file === 'main' ? 'Browser' : 'Settings';
  const pending = () => new Promise(() => {});
  const notices=[];
  const type = vm.runInNewContext(`${source}\n${name}`,{
    View:class {},context:() => '{"locale":"en"}',messages,errorKey,imported,destination,
    nextBrowserChange:pending,nextNavigationTabEvent:pending,nextTextEvent:pending,
    nextControlEvent:pending,modal_closed:pending,toast:value=>notices.push(value),...bindings
  });
  const work = [];
  const cx = {notify() {},spawn(task) { const result = task(cx); work.push(result); return result; }};
  const view = new type(); view.init({},cx);
  return {view,cx,notices,async complete(action) {
    const before = work.length; action(); await Promise.all(work.slice(before));
  }};
}

test('page changes preserve an address draft and selecting a stable tab replaces it', async () => {
  let focused = true, address = '', requests = [];
  const snapshot = (selected,url,cursor) => ({selected,cursor,tabs:[{id:selected,url,title:'Page',loading:false,error:null}],history:[false,false]});
  const {view,cx,complete} = await controller('main',{
    readBrowser:() => snapshot(7,'https://example.com/first','1'),
    createText:value => { address = value; return 'address'; },readText:() => address,
    setText:(_id,value) => { address = value; },isTextFocused:() => focused,focusText:() => { focused = true; },
    browserAction:async action => { requests.push(action); return snapshot(9,'https://example.com/selected','3'); },
    toast() {}
  });
  address = 'unfinished search';
  view.accept(snapshot(7,'https://example.com/redirect','2'),false,cx);
  assert.equal(address,'unfinished search');
  await complete(() => view.perform('select',{id:9},cx));
  assert.equal(address,'https://example.com/selected');
  assert.equal(requests[0].id,9);
  assert.equal(view.selected().id,9);
  focused = false;
  view.accept(snapshot(9,'https://example.com/next','4'),false,cx);
  assert.equal(address,'https://example.com/next');
  view.accept(snapshot(7,'https://example.com/stale','2'),true,cx);
  assert.equal(address,'https://example.com/next');
  assert.equal(view.selected().id,9);
  focused = true;
  address = 'another draft';
  view.accept(snapshot(9,'https://example.com/next','4'),true,cx);
  assert.equal(address,'https://example.com/next');
  view.accept(snapshot(9,'https://example.com/nine','9'),true,cx);
  view.accept(snapshot(9,'https://example.com/ten','10'),true,cx);
  view.accept(snapshot(7,'https://example.com/older','9'),true,cx);
  assert.equal(address,'https://example.com/ten');
  assert.equal(view.state.cursor,'10');
});

test('an uncertain browser action is reported once and never replayed', async () => {
  let calls = 0, notices = [];
  const {view,cx,complete} = await controller('main',{
    readBrowser:() => ({cursor:'1',selected:0,tabs:[{id:0,url:''}],history:[false,false]}),
    createText:() => 'address',readText:() => 'example.com',setText() {},focusText() {},isTextFocused:() => false,
    browserAction:async () => { calls++; throw new Error('browser_unavailable'); },
    toast:notice => notices.push(notice)
  });
  await complete(() => view.navigate(cx));
  assert.equal(calls,1);
  assert.equal(notices.length,1);
  assert.equal(notices[0].message,messages('en').browser_unavailable);
});

test('a blurred address draft survives page status changes and rejected navigation', async () => {
  let address = '', focused = false, requests = [], notices = [];
  const snapshot = (cursor, changes = {}) => ({cursor,selected:3,tabs:[{
    id:3,url:'https://example.com/',title:'Page',loading:false,error:null,...changes
  }],history:[false,false]});
  let result = snapshot('4',{error:'browser_invalid_address'});
  const {view,cx,complete} = await controller('main',{
    readBrowser:() => snapshot('1'),
    createText:value => { address = value; return 'address'; },readText:() => address,
    setText:(_id,value) => { address = value; },isTextFocused:() => focused,focusText() {},
    browserAction:async action => { requests.push(action); return result; },
    toast:notice => notices.push(notice)
  });
  address = 'https://';
  view.accept(snapshot('2',{title:'Updated title'}),false,cx);
  assert.equal(address,'https://');
  view.accept(snapshot('3',{title:'Updated title',loading:true}),false,cx);
  assert.equal(address,'https://');
  await complete(() => view.navigate(cx));
  assert.equal(requests[0].kind,'navigate');
  assert.equal(requests[0].url,'https://');
  assert.equal(address,'https://');
  assert.equal(view.selected().url,'https://example.com/');
  assert.equal(notices.length,1);
  assert.equal(notices[0].message,messages('en').browser_invalid_address);
  focused = true;
  address = 'example.com';
  result = snapshot('5');
  await complete(() => view.navigate(cx));
  assert.equal(requests[1].url,'https://example.com');
  assert.equal(address,'https://example.com/');
});

test('invalid address policy preserves the draft without dispatching navigation', async () => {
  let address = '', requests = [], notices = [];
  const {view,cx,complete} = await controller('main',{
    readBrowser:() => ({cursor:'1',selected:0,tabs:[{id:0,url:'https://example.com/'}],history:[false,false]}),
    createText:value => { address = value; return 'address'; },readText:() => address,
    setText:(_id,value) => { address = value; },isTextFocused:() => false,
    browserAction:async action => { requests.push(action); },
    toast:notice => notices.push(notice)
  });
  for (const invalid of ['', 'javascript:alert(1)', 'file:///tmp/file', 'data:text/html,test']) {
    address = invalid;
    await complete(() => view.navigate(cx));
    assert.equal(address,invalid);
  }
  assert.equal(requests.length,0);
  assert.equal(notices.length,4);
  assert.ok(notices.every(notice => notice.message === messages('en').browser_invalid_address));
});

test('profile import uses the selected opaque ID and reports verified counts', async () => {
  let importedId, persisted;
  const {view,cx,complete,notices} = await controller('settings',{
    readBrowserSettings:() => ({supported:true,enabled:true,persistent:persisted ?? true}),
    setBrowserPersistent:value => { persisted = value; },
    listBrowserProfiles:async () => [{id:'profile-other',name:'Work'},{id:'profile-opaque',name:'Personal'}],
    importBrowserProfile:async id => { importedId = id; return {count:8,skipped:2}; }
  });
  await complete(() => view.scan(cx));
  assert.equal(view.profiles[1].id,'profile-opaque');
  assert.equal(importedId,undefined);
  await complete(() => view.import(view.profiles[1],cx));
  assert.equal(importedId,'profile-opaque');
  assert.equal(view.profiles,null);
  assert.equal(Object.hasOwn(view,'message'),false);assert.equal(notices.length,1);assert.equal(notices[0].message,'Imported 8 cookies, skipped 2');assert.equal(notices[0].kind,'info');
  assert.equal(view.busy,false);
  view.persist(false,cx);
  assert.equal(persisted,false);
  assert.equal(view.state.persistent,false);
});

test('profile errors clear busy state and retain localized failure meaning', async () => {
  const {view,cx,complete,notices} = await controller('settings',{
    readBrowserSettings:() => ({supported:true,enabled:true,persistent:true}),
    listBrowserProfiles:async () => { throw new Error('sailry/ui.listBrowserProfiles: browser_chrome_key_denied'); }
  });
  await complete(() => view.scan(cx));
  assert.equal(view.busy,false);
  assert.equal(view.profiles,null);
  assert.equal(Object.hasOwn(view,'message'),false);assert.equal(notices.length,1);assert.equal(notices[0].message,messages('en').browser_chrome_key_denied);assert.equal(notices[0].kind,'error');
});
