import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/explorer.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));
const cx = {notify(){}};
const entry = (name,kind = 'file') => ({name,kind,size:0});
const page = (path,entries,next = null) => ({path,entries,next,revision:'r1',truncated:!!next,unsupported_names:0});

function fixture(read) {
  const errors = [], calls = [];
  const {Explorer} = vm.runInNewContext(`${source}\n({Explorer})`,{
    listDirectory:async (...args) => { calls.push(args); return read(...args); },
    faultCode:message => { assert.equal(typeof message,'string'); return JSON.parse(message).code; },
  });
  return {explorer:new Explorer(error => errors.push(error)),errors,calls};
}

test('paging appends without aliasing duplicate names and refreshes a changed cursor', async () => {
  let call = 0;
  const next = {revision:'r1',directory:false,name:'a.txt'};
  const {explorer,calls,errors} = fixture(path => {
    if (++call === 1) return page(path,[entry('a.txt')],next);
    if (call === 2) return page(path,[entry('a.txt'),entry('b.txt')],next);
    if (call === 3) throw new Error(JSON.stringify({code:'revision_conflict',message:'Directory changed'}));
    return page(path,[entry('fresh.txt')]);
  });
  await explorer.load('',false,cx);
  await explorer.load('',true,cx);
  assert.deepEqual(plain(explorer.visible()),['a.txt','b.txt']);
  assert.deepEqual(plain(calls[1]),['',next]);
  await explorer.load('',true,cx);
  assert.deepEqual(plain(explorer.visible()),['fresh.txt']);
  assert.deepEqual(errors,['files_directory_changed']);
  assert.equal(calls.length,4);
});

test('range and additive selection exclude nested duplicate action targets', async () => {
  const {explorer} = fixture(path => path ? page(path,[entry('inside.txt')])
    : page(path,[entry('folder','directory'),entry('a.txt'),entry('b.txt')]));
  await explorer.load('',false,cx); await explorer.expand('folder',cx);
  explorer.select('folder'); explorer.select('a.txt',{shift:true});
  assert.deepEqual(plain(explorer.paths()),['a.txt','folder','folder/inside.txt']);
  assert.deepEqual(plain(explorer.targets('folder')),['a.txt','folder']);
  explorer.select('b.txt',{additive:true}); explorer.select('a.txt',{additive:true});
  assert.deepEqual(plain(explorer.paths()),['b.txt','folder','folder/inside.txt']);
  explorer.context('a.txt');
  assert.deepEqual(plain(explorer.paths()),['a.txt']);
  assert.equal(explorer.directory('folder/inside.txt'),'folder');
  explorer.all(); explorer.collapse('folder',cx);
  assert.deepEqual(plain(explorer.paths()),['a.txt','b.txt','folder']);
});

test('invalidation during a directory load coalesces one fresh read', async () => {
  let resolve, count = 0;
  const {explorer} = fixture(path => ++count === 1 ? new Promise(done => {resolve = done;}) : page(path,[entry('new.txt')]));
  const loading = explorer.load('',false,cx);
  await explorer.refresh(cx); await explorer.refresh(cx);
  resolve(page('',[entry('old.txt')])); await loading;
  assert.equal(count,2);
  assert.deepEqual(plain(explorer.visible()),['new.txt']);
});

test('directory placeholders and continuation rows cannot become selected file paths', async () => {
  const next = {revision:'r1',directory:true,name:'folder'};
  const {explorer} = fixture(path => page(path,[entry('folder','directory')],next));
  await explorer.load('',false,cx);
  const items = explorer.items({files_loading:'Reading',files_load_more:'Load more'},() => []);
  assert.equal(items[0].children[0].disabled,true);
  assert.equal(items[1].id,'\0more');
  explorer.select('\0more');
  assert.deepEqual(plain(explorer.paths()),[]);
});

test('watch refresh retains loaded pages and selection using only fresh revision cursors',async () => {
  let revision = 'first';
  const {explorer,calls} = fixture((path,after) => {
    if (after) assert.equal(after.revision,revision);
    const start = after ? Number(after.name) : 0;
    return {...page(path,Array.from({length:500},(_,index) => entry(`file-${String(start + index).padStart(4,'0')}`)),
      {revision,directory:false,name:String(start + 500)}),revision};
  });
  await explorer.load('',false,cx); await explorer.load('',true,cx);
  explorer.select('file-0799');
  revision = 'changed';
  await explorer.refresh(cx);
  assert.equal(explorer.visible().length,1000);
  assert.deepEqual(plain(explorer.paths()),['file-0799']);
  assert.equal(explorer.current,'file-0799');
  assert.deepEqual(plain(calls.slice(2)),[['',null],['',{revision:'changed',directory:false,name:'500'}]]);
  assert.equal(explorer.pages.get('').next.revision,'changed');
});

test('large directory menus do not rescan all visible entries for each row',async () => {
  const {explorer} = fixture(path => page(path,Array.from({length:1500},(_,index) => entry(`file-${index}.txt`))));
  await explorer.load('',false,cx);
  explorer.select('file-799.txt');
  const visible = explorer.visible.bind(explorer);
  let scans = 0;
  explorer.visible = () => {scans++; return visible();};
  const selected = explorer.paths();
  for (const path of visible()) {
    assert.equal(explorer.entry(path).name,path);
    assert.deepEqual(plain(explorer.targets(path,selected)),[path]);
  }
  assert.equal(scans,1);
  assert.equal(explorer.entry('missing.txt'),undefined);
});
