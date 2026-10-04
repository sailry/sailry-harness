import test from 'node:test';
import assert from 'node:assert/strict';
import {review} from '../dev.sailry.platform/host/review.js';

const now = 100*86_400_000;
const settings = {review_after_days:30};
const filter = {project:'project',all_projects:false};
const entry = (id,body,changes={}) => ({summary:{id,project:'project',revision:1,updated_at_ms:now,
  archived:false,title:'Verified constraint',kind:'project',...changes},body});
async function candidates(entries,options={}) {
  const reads=[];
  const hints=await review(entries.map(entry=>entry.summary),options.settings ?? settings,
    options.filter ?? filter,async id=>{reads.push(id);return structuredClone(entries.find(entry=>entry.summary.id===id));},now);
  return {hints,reads};
}

test('uses the exact similarity threshold and newest entry as the consolidation hint',async()=>{
  const entries=[entry('older','alpha beta gamma delta epsilon',{updated_at_ms:now-1}),
    entry('newer','alpha beta gamma delta'),entry('different','alpha beta gamma zeta')];
  const {hints}=await candidates(entries);
  assert.deepEqual(hints,[{summary:entries[0].summary,duplicate_of:'newer',stale:false}]);
  const tied=[entry('b','same shared verified fact'),entry('a','same shared verified fact')];
  assert.equal((await candidates(tied)).hints[0].duplicate_of,'a');
});

test('preserves CJK bigrams, identifier splits and meaningful code distinctions',async()=>{
  for (const [left,right] of [
    ['数据库连接使用连接池，并限制最大连接数','数据库连接使用连接池，并限制最大连接数为十个'],
    ['SessionStore owns durable authoritative session history','session store owns durable authoritative session history'],
    ['HTTPServer owns stable verified durable history','HTTP Server owns stable verified durable history'],
    ['ÉclairStore keeps verified durable history','éclair store keeps verified durable history'],
    ['𠀀𠀁𠀂𠀃𠀄','𠀀𠀁𠀂𠀃𠀄𠀅'],
  ]) {
    assert.equal((await candidates([entry('a',left),entry('b',right)])).hints.length,1,`${left}: ${right}`);
  }
  for (const [left,right] of [
    ['Use C++','Use C#'],['Use version 1.2','Use version 1.3'],['Use cache','Do not use cache'],['中断','中文'],['!!!','???'],
  ]) {
    assert.equal((await candidates([entry('a',left),entry('b',right)])).hints.length,0,`${left}: ${right}`);
  }
});

test('filters reads by active visible scope and never compares different projects',async()=>{
  const entries=[entry('current','Verified durable fact'),entry('global','Verified durable fact',{project:null}),
    entry('foreign','Verified durable fact',{project:'foreign'}),entry('archived','Verified durable fact',{archived:true})];
  assert.deepEqual(await candidates(entries),{hints:[],reads:['current','global']});
  assert.equal((await candidates(entries,{filter:{project:null,all_projects:true}})).hints.length,0);
  assert.deepEqual((await candidates(entries,{filter:{project:null,all_projects:false}})).reads,['global']);
});

test('age is a boundary-checked hint without mutating or removing facts',async()=>{
  const entries=[entry('old','Old verified fact',{updated_at_ms:now-30*86_400_000}),
    entry('recent','Recent constraint',{updated_at_ms:now-30*86_400_000+1}),
    entry('future','Future clock',{updated_at_ms:now+1})];
  const original=structuredClone(entries), {hints}=await candidates(entries);
  assert.deepEqual(hints,[{summary:entries[0].summary,duplicate_of:null,stale:true}]);
  assert.deepEqual(entries,original);
  assert.equal((await candidates(entries,{settings:{review_after_days:31}})).hints.length,0);
});

test('bounds native reads and reports changed revisions without partial hints',async()=>{
  const entries=Array.from({length:20},(_,index)=>entry(String(index),`Distinct verified fact ${index}`));
  let active=0,maximum=0;
  await review(entries.map(entry=>entry.summary),settings,filter,async id=>{
    maximum=Math.max(maximum,++active);
    await new Promise(done=>setImmediate(done));active--;
    return entries.find(entry=>entry.summary.id===id);
  },now);
  assert.equal(maximum,4);
  await assert.rejects(review([entries[0].summary],settings,filter,async()=>({
    ...entries[0],summary:{...entries[0].summary,revision:2},
  }),now),error=>error.code==='revision_conflict');
});

test('reviews a full-size CJK catalog with compact exact fingerprints',async()=>{
  const body=Array.from({length:2730},(_,index)=>String.fromCodePoint(0x4e00+index)).join('');
  const entries=Array.from({length:512},(_,index)=>entry(String(index).padStart(3,'0'),body));
  const {hints,reads}=await candidates(entries);
  assert.equal(reads.length,512);
  assert.equal(hints.length,511);
  assert.ok(hints.every(hint=>hint.duplicate_of==='000' && !hint.stale));
});
