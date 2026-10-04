import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

const settle=async()=>{for(let index=0;index<12;index++)await Promise.resolve();};
async function controller(name,file='main.js',globals={}) {
  const source=(await readFile(new URL(`../${name}/dev.sailry.platform/desktop/${file}`,import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace(/^export /gm,'').replace('default class','class');
  return vm.runInNewContext(`${source}\n${name==='scheduled-tasks'?'ScheduledTasks':name==='reminders'?'Reminders':({databases:'Databases',gomoku:'Gomoku',reversi:'Reversi',xiangqi:'Xiangqi',poker:'Poker','liars-dice':'LiarsDice',doudizhu:'DouDizhu','city-trader':'CityTrader'})[name]}`,{
    View:class{},Motion:class{},context:()=>JSON.stringify({locale:'en'}),
    messages:()=>new Proxy({names:['First','Second','Third']},{get:(value,key)=>value[key]??key}),...globals,
  });
}
function changes() {
  let pending;
  return {read:()=>new Promise((resolve,reject)=>{pending={resolve,reject};}),
    async send(cursor,entry='0',connected=true){assert.ok(pending);pending.resolve({cursor,entry,connected});await settle();},
    async close(){pending.reject(new Error('View closed'));await settle();}};
}

for(const name of ['scheduled-tasks','reminders'])test(`${name}: entry reloads data and retains drafts and requests after failed reads`,async()=>{
  const updates=changes(),jobs=[],notices=[];
  let revision=1,fail=false,reads=0;
  const load=async()=>{reads++;if(fail)throw new Error('Unavailable');return {items:[{id:`item-${revision}`,title:'Retained item',name:'Retained item'}],after:null};};
  const Controller=await controller(name,'main.js',{
    nextChange:updates.read,page:load,list:async()=>({Ok:await load()}),
    history:async()=>({Ok:{items:[{job:{id:`run-${revision}`}}],next_before:null}}),
    states:async()=>({}),
    readProjectCatalog:async()=>({projects:[{id:'project',name:'Project'}],worktrees:[]}),
    listModels:async()=>({models:[]}),toast:value=>notices.push(value),
    prepareRequest:()=>assert.fail('read invalidation must not replay an action'),
  });
  const owner=new Controller(),cx={notify(){},spawn:job=>jobs.push(job)};
  owner.init(null,cx);
  const observation=jobs[0](cx);
  await updates.send('7:true:0');
  await jobs.at(-1)(cx);
  assert.equal(owner.items[0].id,'item-1');
  const items=owner.items,catalog=owner.catalog,editing={draft:{title:'Unsaved draft'}};
  Object.assign(owner,{editing,pending:'original-request',error:'unknown',tab:1,pages:3});
  fail=true;
  await updates.send('7:true:1','1');
  await jobs.at(-1)(cx);
  assert.equal(reads,2);assert.equal(owner.items,items);assert.equal(owner.catalog,catalog);
  assert.equal(owner.editing,editing);assert.equal(owner.pending,'original-request');
  assert.equal(owner.tab,1);assert.equal(owner.pages,3);assert.equal(owner.loading,false);
  assert.equal(notices.length,1);assert.equal(notices[0].message,'loadFailed');
  fail=false;revision=2;
  await updates.send('7:true:2','2');
  await jobs.at(-1)(cx);
  assert.equal(reads,3);assert.equal(owner.items[0].id,'item-2');
  assert.equal(owner.editing,editing);assert.equal(owner.pending,'original-request');
  assert.equal(owner.tab,1);assert.equal(owner.pages,3);assert.equal(notices.length,1);
  await updates.close();await observation;
});

for(const name of ['gomoku','reversi','xiangqi','poker','liars-dice','doudizhu','city-trader'])test(`${name}: entry refreshes the lobby without restarting a game or replaying an uncertain turn`,async()=>{
  const updates=changes(),jobs=[];
  const Controller=await controller(name,'main.js',{nextChange:updates.read});
  const owner=new Controller(),cx={notify(){},spawn:job=>jobs.push(job)};
  let reads=0;
  owner.configure=()=>{reads++;};owner.init(null,cx);
  const observation=jobs.at(-1)(cx);
  await updates.send('7:true:0');assert.equal(reads,1);
  await updates.send('8:true:0');assert.equal(reads,1);
  await updates.send('8:true:1','1');assert.equal(reads,2);
  const game={phase:'retained'};
  Object.assign(owner,{game,pending:'original-turn',error:'unconfirmed'});
  await updates.send('8:true:2','2');assert.equal(reads,2);
  assert.equal(owner.game,game);assert.equal(owner.pending,'original-turn');assert.equal(owner.error,'unconfirmed');
  owner.game=null;
  await updates.send('8:true:3','3');assert.equal(reads,2);
  owner.pending=null;owner.savePending='original-save';
  await updates.send('8:true:4','4');assert.equal(reads,2);
  owner.savePending=null;
  await updates.send('8:true:5','5');assert.equal(reads,3);
  await updates.send('8:false:5','5',false);
  await updates.send('8:true:5','5',true);assert.equal(reads,4);
  await updates.close();await observation;
});

test('databases: entry refreshes the visible catalog without running SQL or discarding selection and drafts',async()=>{
  const jobs=[],loads=[],profile={id:'saved',revision:4};
  const nextChange=()=>{},Controller=await controller('databases','main.js',{
    nextChange,listDatabases:async()=>[profile],listSsh:async()=>[],readProjectCatalog:async()=>({projects:[]}),
    createText:()=> 'sql-draft',nextTableEvent(){},nextTreeEvent(){},nextContextMenuEvent(){},
    header_action(){},nextNavigationTabEvent(){},nextPickerEvent(){},nextControlEvent(){},nextTextEvent(){},modal_closed(){},nextAssistantTools(){},
    Request:class{constructor(){assert.fail('entry must not execute SQL or replay a request');}},
  });
  const owner=new Controller(),cx={notify(){},focus_handle(){},spawn:job=>jobs.push(job)};
  let changed;
  owner.watch=(read,handle)=>{if(read===nextChange)changed=handle;};owner.init(null,cx);
  await jobs.shift()(cx);
  changed({cursor:'7:true:0',entry:'0',connected:true},cx);await jobs.shift()(cx);
  const catalog={profile,loading:new Set(),expanded:new Set(['main']),current:'table:0:0',load:async(database,_cx,reset)=>loads.push([database,reset])};
  const result={rows:[['Retained result']]},editor={draft:'Unsaved connection'};
  Object.assign(owner,{opened:profile.id,catalog,result,editor,tab:1,page:3});
  changed({cursor:'7:true:1',entry:'1',connected:true},cx);await jobs.shift()(cx);
  assert.deepEqual(loads,[[null,undefined],['main',undefined]]);
  assert.equal(owner.catalog,catalog);assert.equal(catalog.current,'table:0:0');assert.ok(catalog.expanded.has('main'));
  assert.equal(owner.result,result);assert.equal(owner.editor,editor);assert.equal(owner.sql,'sql-draft');assert.equal(owner.tab,1);assert.equal(owner.page,3);
  const pending={request:{id:'original-request',unknown:true}};owner.pending=pending;
  changed({cursor:'7:true:2',entry:'2',connected:true},cx);await jobs.shift()(cx);
  assert.equal(loads.length,2);assert.equal(owner.pending,pending);
});
