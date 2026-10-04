import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {review} from '../dev.sailry.platform/host/review.js';

async function load(sdk = {}) {
  const context = vm.createContext({});
  const cache = new Map();
  const names = ['readTurnState','stageTurnState','newId'];
  const bridge = new vm.SyntheticModule(names,function() {
    for (const name of names) this.setExport(name, sdk[name] ?? (() => { throw new Error(`unexpected ${name}`); }));
  }, {context});
  const synthetic = values => new vm.SyntheticModule(Object.keys(values),function() {
    for (const [name,value] of Object.entries(values)) this.setExport(name,value);
  },{context});
  const unexpected = name => sdk[name] ?? (()=>{throw new Error(`unexpected ${name}`);});
  const services = new Map([
    ['settings.js',synthetic({readMemorySettings:unexpected('readMemorySettings')})],
    ['retrieval.js',synthetic({listMemories:unexpected('listMemories'),searchMemories:unexpected('searchMemories'),
      reviewMemories:async(settings,filter)=>review(await unexpected('listMemories')(),settings,filter,unexpected('readMemory'))})],
    ['storage.js',synthetic({readMemory:unexpected('readMemory')})],
    ['mutations.js',synthetic({putOperations:(entry,expected_revision)=>({entry,expected_revision}),
      removeOperations:(id,expected_revision)=>({id,expected_revision}),mergeOperations:(entry,sources)=>({entry,sources}),
      memoryOutput:output=>({kind:'memory_results',data:output.data.map(value=>value.data.value.summary)})})],
  ]);
  async function module(path) {
    if (path === 'sailry/sdk') return bridge;
    if (services.has(path)) return services.get(path);
    if (cache.has(path)) return cache.get(path);
    const source = await readFile(new URL(`../dev.sailry.platform/host/${path}`,import.meta.url),'utf8');
    const value = new vm.SourceTextModule(source,{context,identifier:path});
    cache.set(path,value);
    await value.link(name => module(name.replace('./','')));
    return value;
  }
  const main = await module('main.js');
  await main.evaluate();
  return {main:main.namespace,policy:cache.get('policy.js').namespace};
}
const id = '11111111-1111-4111-8111-111111111111';
const project = '22222222-2222-4222-8222-222222222222';
const settings = {enabled:true,auto_write:true,context_bytes:4096,review_after_days:90};
const summary = {id,project,title:'Feedback',kind:'feedback',revision:1,updated_at_ms:0,archived:false};
const plain = value => JSON.parse(JSON.stringify(value));

test('prioritizes project decisions and confines the index',async () => {
  const {policy} = await load();
  const references = Array.from({length:40},(_,i) => ({...summary,id:`ref-${i}`,kind:'reference'}));
  const text = policy.index([...references,summary,{...summary,id:'foreign',project:'elsewhere'},{...summary,id:'archived',archived:true}],project,settings);
  const catalog = JSON.parse(text.split('Index:\n')[1]);
  assert.deepEqual(catalog[0],summary);
  assert.ok(catalog.length < 40);
  assert.ok(policy.bytes(text) <= 2048);
  assert.ok(catalog.every(entry => !['foreign','archived'].includes(entry.id)));
  assert.ok(!text.includes('Before answering'));
});

test('shares the exact serialized content budget with Unicode and escaped JSON',async () => {
  const {policy} = await load();
  const state = {remaining:1448};
  const entry = {summary,body:'中文🙂"\\\n'.repeat(200)};
  const results = policy.retrieve([entry],state);
  assert.equal(results.length,1);
  const body = JSON.parse(results[0].parts[0].text);
  assert.equal(body.body_truncated,true);
  assert.ok(entry.body.startsWith(body.body));
  assert.ok(Buffer.byteLength(JSON.stringify(results[0])) + 128 + 600 <= 2048);
  assert.equal(policy.retrieve([entry],state).length,0);
});

test('only complete currently visible search results authorize replacement',async () => {
  const {policy} = await load();
  const result = (name,truncated) => ({name,response:{entries:[{role:'user',parts:[{text:JSON.stringify({summary,body:'Keep conditions',body_truncated:truncated})}]}]}});
  for (const results of [[],[result('search_memory',true)],[result('review_memories',false)]]) {
    assert.throws(() => policy.requireRead({reads:policy.reads(results)},id,1),/complete current body/);
  }
  const state = {reads:policy.reads([result('search_memory',false)])};
  policy.requireRead(state,id,1);
  assert.throws(() => policy.requireRead(state,id,2));
  state.reads = policy.reads([]);
  assert.throws(() => policy.requireRead(state,id,1));
});

test('filters tools at initialization and never contributes imported package instructions',async () => {
  let state;
  let current = {...settings,auto_write:false};
  const {main} = await load({readMemorySettings:async()=>current,listMemories:async()=>[summary],stageTurnState:value=>state=plain(value),readTurnState:()=>plain(state)});
  const tools = ['search_memory','review_memories','save_memory','forget_memory','consolidate_memories'];
  let initialized = await main.initialize({tools,project,resources:true});
  assert.deepEqual(plain(initialized.tools),['search_memory','review_memories']);
  assert.ok(initialized.instruction.includes(id));
  assert.equal(state.remaining,settings.context_bytes-Buffer.byteLength(initialized.instruction));
  initialized = await main.initialize({tools:['search_memory'],project,resources:false});
  assert.equal(initialized.instruction,'');
  assert.equal(state.remaining,settings.context_bytes);
  current = {...current,enabled:false};
  initialized = await main.initialize({tools,project,resources:true});
  assert.deepEqual(plain(initialized),{instruction:'',tools:[]});
});

test('search, replacement, scope policy and live revocation share one captured turn state',async () => {
  let state;
  let current = {...settings};
  const original = {summary,body:'Existing complete body'};
  const {main} = await load({readMemorySettings:async()=>current,listMemories:async()=>[summary],searchMemories:async()=>[original],readMemory:async()=>structuredClone(original),stageTurnState:value=>state=plain(value),readTurnState:()=>plain(state),newId:()=>id});
  await main.initialize({tools:['search_memory','save_memory'],project,resources:true});
  const input = {id,expected_revision:1,kind:'feedback',title:'Correction',body:'Updated with conditions'};
  assert.equal((await main.save(input)).error.code,'invalid_request');
  const retrieved = await main.search({query:id});
  main.beforeModel({results:[{name:'search_memory',response:retrieved}]});
  const prepared = await main.save(input);
  assert.equal(prepared.entry.summary.project,project);
  assert.equal(prepared.entry.body,input.body);
  main.beforeModel({results:[]});
  assert.equal((await main.save(input)).error.code,'invalid_request');
  const create = {expected_revision:0,global:true,kind:'project',title:'Invalid global fact',body:'A project fact'};
  assert.equal((await main.save(create)).error.code,'invalid_request');
  current.auto_write = false;
  assert.equal((await main.save({...create,kind:'user'})).error.code,'permission_denied');
});

test('formats mutations without disclosing bodies or replacing authoritative faults',async () => {
  const {main} = await load();
  const output = {kind:'plugin_transaction',data:[{kind:'plugin_value',data:{value:{summary}}}]};
  assert.deepEqual(plain(main.result({output})),{updated:[summary]});
  const failure = {isError:true,error:{code:'outcome_unknown',message:'Uncertain'}};
  assert.equal(main.result({output:failure}),failure);
});

test('package review shares the captured content budget and never grants replacement authority',async () => {
  let state;
  const read = [];
  const entries = [{summary,body:'Use concise Chinese replies'},
    {summary:{...summary,id:'older',updated_at_ms:1},body:'Use concise Chinese replies today'},
    {summary:{...summary,id:'foreign',project:'elsewhere'},body:'Unavailable'}];
  const {main} = await load({readMemorySettings:async()=>settings,listMemories:async()=>entries.map(entry=>entry.summary),
    readMemory:async id=>{read.push(id);return entries.find(entry=>entry.summary.id===id);},
    stageTurnState:value=>state=plain(value),readTurnState:()=>plain(state)});
  await main.initialize({tools:['review_memories'],project,resources:false});
  const output = await main.review({});
  assert.equal(output.candidates.length,2);
  assert.ok(output.candidates.some(candidate=>candidate.duplicate_of==='older'));
  assert.ok(!read.includes('foreign'));
  assert.ok(state.remaining < settings.context_bytes);
  assert.ok(!JSON.stringify(output).includes('Use concise'));
  main.beforeModel({results:[{name:'review_memories',response:output}]});
  assert.deepEqual(state.reads,[]);
  state.remaining = 1;
  assert.deepEqual(plain(await main.review({})),{candidates:[]});
});
