import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

async function fixture(result) {
  const source=(await readFile(new URL('../dev.sailry.platform/desktop/main.js',import.meta.url),'utf8'))
    .replace(/^import .*;\n/gm,'').replace('export default class','class');
  const notices=[],tasks=[],requests=[];
  const Controller=vm.runInNewContext(`${source}\nProjectSummary`,{View:class{},prepare:value=>{requests.push(JSON.parse(value));return 'original-request';},
    execute:async()=>JSON.stringify(result),forget(){},report:()=> 'Retained report',path:'fixture.md',toast:value=>notices.push(value)});
  const owner=new Controller(),status={entries:[{path:'retained.txt'}]};
  Object.assign(owner,{scope:{worktree:'captured'},text:new Proxy({},{get:(_,key)=>key}),include:true,status,revision:'prior',busy:false,pending:null,canSave:true,changed:true});
  const cx={notify(){},spawn:work=>tasks.push(work(cx))};
  return {owner,status,notices,requests,cx,settle:()=>Promise.all(tasks)};
}

test('save success and failure use toasts without replacing the previous report',async()=>{
  for(const result of [{Ok:{kind:'file_written',data:{revision:'saved'}}},{Err:{code:'revision_conflict'}},{Err:{code:'outcome_unknown'}}]) {
    const setup=await fixture(result);setup.owner.save(setup.cx);await setup.settle();
    assert.equal(setup.owner.status,setup.status);assert.equal(setup.owner.include,true);assert.equal(setup.owner.changed,true);
    assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].message,result.Ok?'saved':result.Err.code==='revision_conflict'?'conflict':'unknown');
    assert.equal(setup.notices[0].kind,result.Ok?'info':'error');assert.equal(setup.requests[0].data.worktree,'captured');
    assert.equal(setup.owner.pending,result.Err?.code==='outcome_unknown'?'original-request':null);
  }
});
