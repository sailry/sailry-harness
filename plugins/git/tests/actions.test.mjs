import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/actions.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');

function fixture(receipts = []) {
  const fields=new Map(),prepared=[],completed=[],forgotten=[],released=[],reported=[];
  let serial=0,refreshes=0;
  const Actions=vm.runInNewContext(`${source}\nActions`,{
    createText:value=>{const id=`field-${++serial}`;fields.set(id,value);return id;},
    readText:id=>fields.get(id),releaseText:id=>released.push(id),focusText:()=>{},
    prepareGitChange:draft=>{prepared.push(JSON.parse(JSON.stringify(draft)));return 'captured-request';},
    completeRequest:async id=>{completed.push(id);return receipts.shift();},
    forgetRequest:id=>forgotten.push(id),faultCode:()=> 'outcome_unknown',
  });
  const view={text:{},notify:()=>{},report:key=>reported.push(key),repo:{
    status:{head:'captured-head',branch:'main'},
    revisions:()=>({expected_head:'captured-head',expected_index:'captured-index'}),
    refresh:async()=>{refreshes++;},
  }};
  return {owner:new Actions(view),fields,prepared,completed,forgotten,released,reported,refreshes:()=>refreshes};
}

test('empty names report validation without replacing the form or draft',async()=>{
  for(const form of ['branch','rename','tag']) {
    const state=fixture();
    state.owner.form(form,{name:'existing',commit:'captured-head'});
    const dialog=state.owner.dialog,input=dialog.input;
    state.fields.set(input,'');
    if(dialog.extra)state.fields.set(dialog.extra,'Keep tag message');
    await state.owner.submit();
    assert.equal(state.owner.dialog,dialog);
    assert.equal(dialog.input,input);
    assert.equal(state.fields.get(input),'');
    if(dialog.extra)assert.equal(state.fields.get(dialog.extra),'Keep tag message');
    assert.equal(dialog.error,'git_branch_name_required');
    assert.deepEqual(state.reported,['git_branch_name_required']);
    assert.deepEqual(state.prepared,[]);
    assert.deepEqual(state.completed,[]);
    assert.deepEqual(state.forgotten,[]);
    assert.deepEqual(state.released,[]);
  }
});

test('unconfirmed receipts report and retry the captured request without losing input',async()=>{
  const state=fixture([{}, {Ok:{kind:'git_change',data:{}}}]);
  state.owner.form('branch');
  const dialog=state.owner.dialog;
  state.fields.set(dialog.input,'topic');
  await state.owner.submit();
  assert.equal(state.owner.dialog,dialog);
  assert.equal(state.fields.get(dialog.input),'topic');
  assert.equal(dialog.pending.id,'captured-request');
  assert.equal(dialog.busy,false);
  assert.equal(dialog.error,'git_branch_create_unknown');
  assert.deepEqual(state.reported,['git_branch_create_unknown']);
  assert.deepEqual(state.prepared,[{kind:'create',name:'topic',commit:'captured-head'}]);
  assert.deepEqual(state.completed,['captured-request']);
  assert.deepEqual(state.forgotten,[]);
  assert.deepEqual(state.released,[]);
  await state.owner.submit();
  assert.equal(state.owner.dialog,null);
  assert.equal(state.prepared.length,1);
  assert.deepEqual(state.completed,['captured-request','captured-request']);
  assert.deepEqual(state.forgotten,['captured-request']);
  assert.deepEqual(state.released,[dialog.input]);
  assert.equal(state.refreshes(),1);
});
