import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const root = new URL('../',import.meta.url);
const source = (await readFile(new URL('dev.sailry.platform/desktop/main.js',root),'utf8'))
  .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
const actionSource = (await readFile(new URL('dev.sailry.platform/desktop/actions.js',root),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');

test('completed directory work prepares menus before notifying the renderer', async () => {
  const Files = vm.runInNewContext(`${source}\nFiles`,{View:class {}});
  const view = new Files(), events = [], tasks = [];
  const cx = {spawn:task => tasks.push(task(cx)),notify:() => events.push('notify')};
  view.actions = {prepareTree:() => events.push('menus')};
  view.run(async () => events.push('directory'),cx);
  await Promise.all(tasks);
  assert.deepEqual(events,['directory','menus','notify']);
});

test('native search options retain package-owned control state', () => {
  const Files = vm.runInNewContext(`${source}\nFiles`,{View:class {}});
  const view = new Files(), calls = [], cx = {};
  view.search = {toggle:(option,eventCx) => calls.push([option,eventCx])};
  view.control('file-search-case',cx);
  view.control('file-search-regex',cx);
  assert.deepEqual(calls,[['case_sensitive',cx],['regex',cx]]);
  view.search = null;
  view.control('file-search-case',cx);
  view.control('file-search-regex',cx);
  assert.equal(calls.length,2);
});

test('artifact resources mount only the core preview and start no workspace controllers', () => {
  const fail = () => assert.fail('workspace API must not run for a captured artifact');
  const Files = vm.runInNewContext(`${source}\nFiles`,{
    View:class {},fileResource:() => ({path:'report.pdf'}),context:fail,workspaceMode:fail,
    Documents:fail,Explorer:fail,Transfers:fail,FilePreview:{new:id => ({id})},
  });
  const view = new Files(); view.init({}, {spawn:fail});
  assert.deepEqual(JSON.parse(JSON.stringify(view.render())),{id:'file-preview'});
});

test('file activation preserves native image priority and document line targets', async () => {
  const calls = [];
  const Files = vm.runInNewContext(`${source}\nFiles`,{
    View:class {},inspectFilePath:path => ({image:path.endsWith('.png'),external:/\.(png|pdf)$/.test(path)}),
    openFileImage:path => calls.push(['image',path]),openSystem:async path => calls.push(['system',path]),
  });
  const view = new Files(); view.documents = {open:async (path,line) => calls.push(['document',path,line])};
  await view.open('image.png',undefined,{});
  await view.open('report.pdf',undefined,{});
  await view.open('source.rs',21,{});
  assert.deepEqual(calls,[['image','image.png'],['system','report.pdf'],['document','source.rs',21]]);
});

test('unclassified binary files open in the system while access failures retain their fault', async () => {
  const opened = [];
  const Files = vm.runInNewContext(`${source}\nFiles`,{
    View:class {},inspectFilePath:() => ({image:false,external:false}),
    openSystem:async path => opened.push(path),
    faultCode:message => { assert.equal(typeof message,'string'); return JSON.parse(message).code; },
  });
  const view = new Files();
  view.documents = {open:async path => {
    throw new Error(JSON.stringify({code:path === 'binary.data' ? 'invalid_request' : 'permission_denied',message:'Cannot read'}));
  }};
  await view.open('binary.data',undefined,{});
  await assert.rejects(view.open('private.data',undefined,{}),/permission_denied/);
  assert.deepEqual(opened,['binary.data']);
});

test('the semantic close action uses the selected draft flow before closing an empty panel', () => {
  const closed = [];
  const Files = vm.runInNewContext(`${source}\nFiles`,{View:class {},closePanel:() => closed.push('panel')});
  const view = new Files();
  let selected = {id:'dirty'};
  view.documents = {current:() => selected,close:id => closed.push(id)};
  view.run = action => action();
  view.control('files-close-current',{});
  selected = null;
  view.control('files-close-current',{});
  assert.deepEqual(closed,['dirty','panel']);
});

test('native events and asynchronous document actions use the spawned context', async () => {
  const tasks = [], calls = [];
  const ambient = {notify() {},spawn:task => { tasks.push(task(ambient)); }};
  const caller = {spawn:task => { tasks.push(task(ambient)); },notify() {
    assert.fail('an earlier call context must not be retained');
  }};
  const event = value => {
    let delivered = false;
    return async () => {
      if (delivered) throw new Error('stream closed');
      delivered = true;
      return value;
    };
  };
  const stopped = async () => { throw new Error('stream closed'); };
  const record = (kind,cx) => { assert.equal(cx,ambient); calls.push(kind); };
  const Files = vm.runInNewContext(`${source}\nFiles`,{
    View:class {},fileResource:() => null,workspaceMode:() => 'main',context:() => '{"locale":"en"}',
    messages:() => ({}),canInsertFileReferences:async () => false,next_change:stopped,
    nextControlEvent:event({id:'files_find'}),
    nextNavigationTabEvent:event({bar:'file-tabs',id:'draft',kind:'close'}),
    nextMenuEvent:event({id:'new-file'}),nextContextMenuEvent:stopped,
    nextTreeEvent:stopped,nextTextEvent:stopped,modal_closed:stopped,
    Documents:class {
      observe() {}
      action(kind,cx) { record(kind,cx); }
      close(_id,cx) { record('close',cx); }
    },
    Explorer:class { refresh(cx) { record('refresh',cx); } },
    Transfers:class { observe() {} },
    Actions:class { prepareTree() {} invoke(_id,cx) { record('menu',cx); } },
  });
  const view = new Files();
  view.report = key => assert.fail(`unexpected action error: ${key}`);
  view.init({},caller);
  for (let index = 0; index < tasks.length; index++) await tasks[index];
  assert.deepEqual(calls.sort(),['close','find','menu','refresh']);
});

test('native menu selection retains its original targets and ignores closed document targets', async () => {
  const copied = [],opened = [];
  const Actions = vm.runInNewContext(`${actionSource}\nActions`,{
    writeClipboard:text => copied.push(text),openSystem:async path => opened.push(path),
  });
  let docs = [{id:'one',path:'old.txt'}];
  const view = {text:{files_copy_path:'Copy path',files_open_system:'Open'},
    documents:{items:() => docs},explorer:{connected:true}};
  const actions = new Actions(view), entries = actions.path(docs[0]);
  docs = [{id:'one',path:'old.txt'},{id:'two',path:'new.txt'}];
  await actions.invoke(entries[1].id,{});
  await actions.invoke(entries[0].id,{});
  assert.deepEqual(copied,['old.txt']); assert.deepEqual(opened,['old.txt']);
  docs = [docs[1]];
  await actions.invoke(entries[1].id,{});
  assert.deepEqual(copied,['old.txt']);
});

test('the shipped workspace and preview metadata includes every imported resource', async () => {
  const manifest = JSON.parse(await readFile(new URL('plugin.json',root),'utf8'));
  const desktop = manifest.extensions['dev.sailry.platform'].desktop;
  assert.deepEqual(desktop.navigation_options,{pinned:true,order:400,target:'worktree',details:true});
  assert.deepEqual(desktop.renderers,[{resource:'documents',shortcut:'secondary-p'}]);
  assert.equal(desktop.previews.length,5);
  let bytes = 0;
  for (const path of desktop.resources) {
    const content = await readFile(new URL(path,root),'utf8'); bytes += Buffer.byteLength(content);
    for (const [,relative] of content.matchAll(/from ['"]\.\/(.*?)['"]/g)) {
      assert.ok(desktop.resources.includes(`dev.sailry.platform/desktop/${relative}`),`${path} imports unlisted ${relative}`);
    }
  }
  assert.ok(bytes <= 256 * 1024);
});
