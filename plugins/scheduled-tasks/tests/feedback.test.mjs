import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';

async function fixture(packageName,outcome,validation=null) {
  const source=(await readFile(new URL(`../../${packageName}/dev.sailry.platform/desktop/main.js`,import.meta.url),'utf8'))
    .replace(/^import[\s\S]*?;\n/gm,'').replace('export default class','class');
  const notices=[],requests=[],forgotten=[],tasks=[],name=packageName==='reminders'?'Reminders':'ScheduledTasks';
  const Controller=vm.runInNewContext(`${source}\n${name}`,{View:class{},toast:value=>notices.push(value),
    prepareRequest:value=>{requests.push(value);return 'original-request';},completeRequest:async()=>outcome,forgetRequest:id=>forgotten.push(id),
    value:editor=>{if(validation)throw new Error(validation);return editor.draft;}});
  const owner=new Controller(),draft={title:'Retained reminder',name:'Retained task'},editing={draft},items=[{id:'saved'}];
  Object.assign(owner,{text:new Proxy({},{get:(_,key)=>key}),editing,items,busy:false,pending:null,error:null});
  owner.refresh=()=>{};owner.close=()=>assert.fail('failed operations must keep their editor');
  const cx={notify(){},spawn:work=>tasks.push(work(cx))};
  return {owner,draft,editing,items,notices,requests,forgotten,cx,settle:()=>Promise.all(tasks)};
}

for(const packageName of ['reminders','scheduled-tasks']) {
  test(`${packageName} validation failures toast once and retain the draft`,async()=>{
    const setup=await fixture(packageName,null,'required');setup.owner.save(setup.cx);await setup.settle();
    assert.equal(setup.owner.editing,setup.editing);assert.equal(setup.owner.editing.draft,setup.draft);assert.equal(setup.owner.items,setup.items);
    assert.equal(setup.requests.length,0);assert.equal(setup.notices.length,1);assert.equal(setup.notices[0].message,'required');assert.equal(setup.notices[0].kind,'error');
  });

  test(`${packageName} conflict and uncertainty preserve drafts and recovery ownership`,async()=>{
    for(const code of ['revision_conflict','outcome_unknown']) {
      const setup=await fixture(packageName,{Err:{code}});setup.owner.save(setup.cx);await setup.settle();
      assert.equal(setup.owner.editing,setup.editing);assert.equal(setup.owner.items,setup.items);assert.equal(setup.notices.length,1);
      assert.equal(setup.notices[0].message,code==='outcome_unknown'?'unknown':packageName==='reminders'?'failed':'conflict');
      assert.equal(setup.notices[0].kind,'error');assert.equal(setup.owner.pending,code==='outcome_unknown'?'original-request':null);
      assert.equal(setup.requests.length,1);
    }
  });
}
