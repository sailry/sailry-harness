import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await Promise.all(['actions','explorer'].map(name =>
  readFile(new URL(`../dev.sailry.platform/desktop/${name}.js`,import.meta.url),'utf8'))))
  .map(value => value.replace(/^import .*;\n/gm,'').replace(/^export /gm,''))
  .join('\n');
const {Actions,Explorer} = vm.runInNewContext(`${source}\n({Actions,Explorer})`);
const plain = value => JSON.parse(JSON.stringify(value));

function fixture(count = 3) {
  const explorer = new Explorer(error => assert.fail(error));
  const entries = Array.from({length:count},(_,index) => ({name:`file-${index}.txt`,kind:'file'}));
  explorer.pages.set('',{entries,index:new Map(entries.map(entry => [entry.name,entry]))});
  const view = {explorer,canInsert:false,text:{},transfers:{canPaste:() => false}};
  return {view,actions:new Actions(view)};
}

function item(actions,path,action) {
  return actions.treeMenu()(path).find(item => actions.entries.get(item.id)?.action === action);
}

test('large directory renders reuse prepared menu targets', () => {
  const {view,actions} = fixture(1_100);
  let builds = 0;
  const build = actions.tree.bind(actions);
  actions.tree = (...args) => { builds++; return build(...args); };
  actions.prepareTree();
  assert.equal(builds,1_100);
  const first = actions.treeMenu()('file-0.txt');
  for (let render = 0; render < 3; render++) {
    const rows = view.explorer.items({},actions.treeMenu());
    assert.equal(rows.length,1_100);
    assert.equal(rows[0].menu,first);
    assert.equal(rows[1_099].menu.length,first.length);
  }
  assert.equal(builds,1_100);
  assert.deepEqual(plain(actions.entries.get(item(actions,'file-0.txt','copy').id)),{
    action:'copy',target:{paths:['file-0.txt'],directory:''},
  });
});

test('selection refresh preserves earlier captured actions', () => {
  const {view,actions} = fixture();
  actions.prepareTree();
  const captured = item(actions,'file-0.txt','copy').id;
  view.explorer.select('file-1.txt');
  view.explorer.select('file-2.txt',{shift:true});
  actions.prepareTree();
  assert.equal(actions.treeMenu()('file-1.txt'),actions.treeMenu()('file-2.txt'));
  assert.deepEqual(plain(actions.entries.get(item(actions,'file-1.txt','copy').id)),{
    action:'copy',target:{paths:['file-1.txt','file-2.txt'],directory:''},
  });
  assert.deepEqual(plain(actions.entries.get(captured)),{
    action:'copy',target:{paths:['file-0.txt'],directory:''},
  });
});

test('menu refresh follows access and clipboard state', () => {
  const {view,actions} = fixture();
  actions.prepareTree();
  assert.equal(item(actions,'file-0.txt','paste').enabled,false);
  assert.equal(item(actions,'file-0.txt','insert').enabled,false);
  view.canInsert = true;
  view.transfers.canPaste = () => true;
  actions.prepareTree();
  assert.equal(item(actions,'file-0.txt','paste').enabled,true);
  assert.equal(item(actions,'file-0.txt','insert').enabled,true);
  view.explorer.connected = false;
  actions.prepareTree();
  assert.equal(item(actions,'file-0.txt','paste').enabled,false);
  assert.equal(item(actions,'file-0.txt','trash').enabled,false);
});
