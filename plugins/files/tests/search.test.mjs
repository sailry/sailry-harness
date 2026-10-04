import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = (await readFile(new URL('../dev.sailry.platform/desktop/search.js',import.meta.url),'utf8'))
  .replace(/^import .*;\n/gm,'').replace(/^export /gm,'');
const cx = {notify(){}};
function fixture() {
  const fields = new Map(), calls = [], errors = [], released = [];
  let cancelled = 0;
  const Search = vm.runInNewContext(`${source}\nSearch`,{
    createText:value => {const id = `field-${fields.size}`; fields.set(id,value); return id;},
    readText:id => fields.get(id),releaseText:id => released.push(id),focusText(){},
    cancelFileSearch:() => cancelled++,faultCode:message => { assert.equal(typeof message,'string'); return JSON.parse(message).code; },
    searchFiles:options => new Promise((resolve,reject) => calls.push({options:JSON.parse(JSON.stringify(options)),resolve,reject})),
  });
  return {search:new Search({},key => errors.push(key)),fields,calls,errors,released,cancelled:() => cancelled};
}

test('editing cancels the old search and its late response cannot replace current results', async () => {
  const f = fixture(); f.fields.set(f.search.query,'old');
  const old = f.search.run(cx);
  f.search.reset(cx); f.fields.set(f.search.query,'new');
  const current = f.search.run(cx);
  f.calls[1].resolve({matches:[{path:'new.txt',line_number:7,line:'new'}],skipped:0,truncated:false});
  await current;
  f.calls[0].resolve({matches:[{path:'old.txt',line_number:1,line:'old'}],skipped:0,truncated:false});
  await old;
  assert.equal(f.search.result.matches[0].path,'new.txt');
  assert.equal(f.cancelled(),3);
});

test('search passes literal expressions and trimmed nonempty glob lines', async () => {
  const f = fixture(); f.fields.set(f.search.query,'资料.*'); f.fields.set(f.search.filter,' **/*.rs \n\n !vendor/**\n');
  f.search.toggle('regex',cx); f.search.toggle('case_sensitive',cx);
  const run = f.search.run(cx);
  assert.deepEqual(f.calls[0].options,{query:'资料.*',regex:true,case_sensitive:true,globs:['**/*.rs','!vendor/**']});
  f.calls[0].resolve({matches:[],skipped:2,truncated:false}); await run;
  assert.equal(f.search.status,'file_search_partial');
  f.search.close(cx); assert.deepEqual(f.released,[f.search.query,f.search.filter]);
});

test('empty queries do not execute and invalid expressions preserve editable input', async () => {
  const f = fixture(); await f.search.run(cx); assert.equal(f.calls.length,0);
  f.fields.set(f.search.query,'['); const run = f.search.run(cx);
  f.calls[0].reject(new Error(JSON.stringify({code:'invalid_request',message:'Invalid expression'}))); await run;
  assert.deepEqual(f.errors,['file_search_invalid']);
  assert.equal(f.fields.get(f.search.query),'[');
  assert.equal(f.search.running,false);
});
