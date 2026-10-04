import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/mutations.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const plain = value => JSON.parse(JSON.stringify(value));
const cx = {notify(){}};

function fixture(results,prepareError) {
  const prepared = [], called = [], forgotten = [];
  const Mutation = vm.runInNewContext(`${source}\nMutation`,{
    parent:path => path.includes('/') ? path.slice(0,path.lastIndexOf('/')) : '',
    join:(parent,name) => parent ? `${parent}/${name}` : name,
    prepareFileAction:args => {if (prepareError) throw prepareError; prepared.push(plain(args)); return `request-${prepared.length}`;},
    completeRequest:async id => {called.push(id); const result = results.shift(); if (result instanceof Error) throw result; return result;},
    forgetRequest:id => forgotten.push(id),faultCode:message => { assert.equal(typeof message,'string'); return JSON.parse(message).code; },
  });
  return {Mutation,prepared,called,forgotten};
}

test('file creation owns empty contents and rejects folder separators before admission', async () => {
  const {Mutation,prepared} = fixture([{Ok:{kind:'file_written'}}]);
  const operation = new Mutation('create',['notes']);
  await operation.submit('nested/file.txt',cx);
  assert.equal(prepared.length,0); assert.equal(operation.error,'files_name_required');
  await operation.submit('资料.txt',cx);
  assert.deepEqual(prepared,[{kind:'write',path:'notes/资料.txt',text:'',revision:null}]);
  assert.equal(operation.done,true);
});

test('a denied preparation reports the structured fault without retaining an unadmitted request', async () => {
  const {Mutation,called,prepared} = fixture([],new Error(JSON.stringify({code:'permission_denied',message:'Read only'})));
  const operation = new Mutation('rename',['old.txt']);
  await operation.submit('new.txt',cx);
  assert.equal(operation.error,'files_rename_denied');
  assert.equal(operation.pending,null);
  assert.deepEqual(called,[]); assert.deepEqual(prepared,[]);
});

test('unknown receipt retries the same frozen rename despite edited draft text', async () => {
  const {Mutation,prepared,called,forgotten} = fixture([new Error('receipt lost'),{Ok:{kind:'entry_renamed'}}]);
  const operation = new Mutation('rename',['folder/old.txt']);
  await operation.submit('new.txt',cx);
  assert.equal(operation.error,'files_rename_unknown');
  assert.deepEqual(forgotten,[]);
  await operation.submit('different.txt',cx);
  assert.deepEqual(prepared,[{kind:'rename',from:'folder/old.txt',to:'folder/new.txt'}]);
  assert.deepEqual(called,['request-1','request-1']);
  assert.equal(operation.done,true);
});

test('sequential deletion resumes only the remaining captured targets', async () => {
  const {Mutation,called,prepared} = fixture([{Ok:{}},new Error('receipt lost'),{Ok:{}}]);
  const operation = new Mutation('trash',['one.txt','two.txt']);
  await operation.submit('',cx);
  assert.equal(operation.completed.length,1);
  await operation.submit('',cx);
  assert.deepEqual(called,['request-1','request-2','request-2']);
  assert.deepEqual(prepared,[{kind:'trash',path:'one.txt'},{kind:'trash',path:'two.txt'}]);
  assert.equal(operation.done,true);
});

test('definite conflicts release the request while terminal uncertainty remains pinned', async () => {
  const {Mutation,forgotten,prepared} = fixture([{Err:{code:'conflict'}},{Err:{code:'outcome_unknown'}},{Err:{code:'outcome_unknown'}}]);
  const operation = new Mutation('create',[''],true);
  await operation.submit('one',cx); assert.equal(operation.error,'files_entry_exists');
  assert.deepEqual(forgotten,['request-1']);
  await operation.submit('two',cx); await operation.submit('three',cx);
  assert.equal(prepared.length,2); assert.equal(prepared[1].path,'two');
  assert.equal(operation.error,'files_create_unknown');
  assert.equal(operation.done,false);
});
