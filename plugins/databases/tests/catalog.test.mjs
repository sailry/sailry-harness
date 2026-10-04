import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

async function fixture(outcome) {
  const source=(await readFile(new URL('../dev.sailry.platform/desktop/catalog.js',import.meta.url),'utf8')).replace(/^import.*\n/,'').replace('export class','class');
  const calls=[],errors=[];
  const Catalog=vm.runInNewContext(`${source}\nCatalog`,{browseDatabase:async(...args)=>{calls.push(args);return typeof outcome==='function'?outcome():outcome;}});
  return {catalog:new Catalog({id:'captured',revision:4},error=>errors.push(error)),calls,errors,cx:{notify(){}}};
}

test('initial table catalogs remain folded until an explicit tree expansion',async()=>{
  const setup=await fixture({kind:'catalog',data:{kind:'tables',data:{database:'main',tables:[{schema:'main',name:'items'}]}}});
  await setup.catalog.load(null,setup.cx);
  const items=setup.catalog.items({},()=>[]);
  assert.equal(items[0].expanded,false);assert.equal(items[0].children[0].label,'items');assert.equal(setup.catalog.current,null);
  assert.deepEqual(setup.calls,[['captured',4,null]]);
  setup.catalog.expanded.add('main');setup.catalog.current='db:0';await setup.catalog.load('main',setup.cx);
  assert.equal(setup.catalog.items({},()=>[])[0].expanded,true);assert.equal(setup.catalog.current,'db:0');
});

test('failed refresh reports once and retains the existing catalog and selection',async()=>{
  const failure=new Error('Catalog unavailable'),setup=await fixture(()=>{throw failure;}),catalog=setup.catalog;
  catalog.names=['main'];catalog.tables.set('main',[{name:'retained',schema:'main'}]);catalog.expanded.add('main');catalog.current='table:0:0';
  await catalog.load(null,setup.cx,true);
  assert.deepEqual(setup.errors,[failure]);assert.deepEqual(catalog.names,['main']);assert.equal(catalog.tables.get('main')[0].name,'retained');
  assert.equal(catalog.expanded.has('main'),true);assert.equal(catalog.current,'table:0:0');assert.equal(catalog.failed.has(''),true);assert.equal(catalog.loading.size,0);
});

test('a failed unloaded folder offers retry instead of a failure message',async()=>{
  const setup=await fixture(()=>{throw new Error('Catalog unavailable');});setup.catalog.names=['main'];
  await setup.catalog.load('main',setup.cx);
  const child=setup.catalog.items({refresh:'Refresh',db_catalog_failed:'Must not appear'},()=>[])[0].children[0];
  assert.equal(child.id,'retry:0');assert.equal(child.label,'Refresh');assert.equal(child.disabled,false);
  assert.equal(setup.catalog.target(child.id).database,'main');assert.equal(setup.errors.length,1);
});

test('empty and unloaded database folders retain disabled placeholder children',async()=>{
  const setup=await fixture({kind:'catalog',data:{kind:'databases',data:['empty','unloaded']}});
  await setup.catalog.load(null,setup.cx);setup.catalog.tables.set('empty',[]);
  const items=setup.catalog.items({db_tables_empty:'Empty',db_catalog_loading:'Loading'},()=>[]);
  assert.equal(items.every(item=>!item.expanded),true);
  assert.equal(items[0].children[0].id,'empty:0');assert.equal(items[0].children[0].disabled,true);
  assert.equal(items[1].children[0].id,'loading:1');assert.equal(items[1].children[0].disabled,true);
});
