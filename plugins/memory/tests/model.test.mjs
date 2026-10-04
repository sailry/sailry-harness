import test from 'node:test';
import assert from 'node:assert/strict';
import {capacity,catalog,equivalence,next,normalized,validate} from '../dev.sailry.platform/host/model.js';
import {terms,query} from '../dev.sailry.platform/host/tokenize.js';

const entry = {summary:{id:'11111111-1111-4111-8111-111111111111',project:null,title:'Reply preference',kind:'user',revision:0,
  updated_at_ms:0,archived:false},body:'Use concise replies'};

test('normalization changes only whitespace and case',()=>{
  assert.equal(normalized('Use  concise\nreplies'),normalized('use concise replies'));
  assert.equal(normalized('Use\u0085concise\u3000replies'),'use concise replies');
  assert.equal(normalized('Use\ufeffreplies'),'use\ufeffreplies');
  for (const [left,right] of [['Use C++','Use C#'],['Use version 1.2','Use version 1.3'],['Use cache','Do not use cache']]) {
    assert.notEqual(normalized(left),normalized(right));
    assert.notEqual(equivalence(left),equivalence(right));
  }
  assert.equal(equivalence('Use  concise\nreplies'),equivalence('use concise replies'));
});

test('validates private entries before creating a new revision',()=>{
  const saved=next(entry,1234);
  assert.equal(saved.summary.revision,1);
  assert.equal(saved.summary.updated_at_ms,1234);
  assert.equal(entry.summary.revision,0);
  for (const invalid of [
    {...entry,body:' '},{...entry,body:'\0'},{...entry,body:'a'.repeat(8193)},
    {...entry,summary:{...entry.summary,title:'Bad\u0085title'}},
    {...entry,summary:{...entry.summary,revision:1.5}},
    {...entry,summary:{...entry.summary,project:'elsewhere'}},
  ]) assert.throws(()=>validate(invalid),error=>error.code==='invalid_request');
});

test('separates live archive capacity and rejects incompatible catalogs without replacement',()=>{
  const head=catalog(null);
  assert.deepEqual(head,{v:1,pages:{},active:0,archived:0,next:0});
  assert.deepEqual(capacity({...head,active:512},[{before:{archived:false},after:{archived:true}}]),{active:511,archived:1});
  assert.throws(()=>capacity({...head,active:512},[{before:null,after:{archived:false}}]),error=>error.code==='busy');
  assert.throws(()=>capacity({...head,archived:2048},[{before:{archived:false},after:{archived:true}}]),error=>error.code==='busy');
  for (const value of [{v:2},{...head,pages:{'1':3}},{...head,active:513},{...head,pages:{'1':1}}]) {
    const original=structuredClone(value);
    assert.throws(()=>catalog(value),/incompatible/);
    assert.deepEqual(value,original);
  }
});

test('prepares literal query terms while preserving Han concepts and identifiers',()=>{
  assert.deepEqual(terms('SessionStore HTTPServer C++ C#'),['session','store','sessionstore','http','server','httpserver','c++','c#']);
  assert.deepEqual(query('如何连接池'),['何连','接池','连接']);
  assert.deepEqual(query('the and of'),[]);
  assert.deepEqual(query('???'),[]);
  assert.deepEqual(query('"Ownership" OR ('),['ownership']);
});
