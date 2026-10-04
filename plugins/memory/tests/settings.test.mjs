import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {review} from '../dev.sailry.platform/host/review.js';

const plain = value => JSON.parse(JSON.stringify(value));
const pending = () => new Promise(() => {});
const defaults = {revision:0,enabled:true,auto_write:true,context_bytes:8192,review_after_days:90};
const id = '11111111-1111-4111-8111-111111111111';
const entry = {summary:{id,project:null,title:'Original',kind:'feedback',revision:1,updated_at_ms:0,archived:false},body:'Preserve original exceptions'};

async function fixture() {
  const fields = new Map(), options = new Map(), prepared = [], completed = [], forgotten = [], jobs = [],notices=[];
  let count = 0, saved = {...defaults}, read = structuredClone(entry), catalog = [plain(entry.summary)];
  let complete = async () => ({Ok:{kind:'plugin_transaction',data:{outcome:{kind:'memory_results'}}}});
  let browse = async () => [plain(entry.summary)];
  const sdk = {
    readMemorySettings:async()=>plain(saved), prepareMemorySettings:value=>prepare('settings',value),
    listMemories:async()=>plain(catalog), browseMemories:async()=>browse(),
    readMemory:async id=>plain(typeof read === 'function' ? await read(id) : read),prepareMemory:(value,revision)=>prepare('put',{value,revision}),
    prepareRemoveMemory:(id,revision)=>prepare('remove',{id,revision}), prepareMergeMemories:(value,sources)=>prepare('merge',{value,sources}),
    newId:()=>id,readProjectCatalog:async()=>({projects:[],worktrees:[]}),
    completeRequest:async id=>{completed.push(id);return complete(id);},forgetRequest:id=>forgotten.push(id),nextChange:pending,
  };
  function prepare(kind,value) { const id=`request-${prepared.length}`; prepared.push({id,kind,value:plain(value)});return id; }
  const forms = {createText:(value,props)=>{const id=`text-${count++}`;fields.set(id,value);options.set(id,plain(props));return id;},readText:id=>fields.get(id),setText:(id,value)=>fields.set(id,value),releaseText:id=>fields.delete(id),nextTextEvent:pending};
  const context=vm.createContext({});
  function synthetic(values) {return new vm.SyntheticModule(Object.keys(values),function(){for(const [key,value] of Object.entries(values))this.setExport(key,value);},{context});}
  const modules = new Map([
    ['gpui-kit',synthetic({View:class{}})],['sailry',synthetic({context:()=>JSON.stringify({locale:'en'})})],
    ['sailry/sdk',synthetic(sdk)],['sailry/forms',synthetic(forms)],['sailry/ui',synthetic({modal_closed:pending,nextControlEvent:pending,toast:value=>notices.push(value)})],
    ['./view.js',synthetic({render:()=>null})],
    ['../host/settings.js',synthetic({readMemorySettings:sdk.readMemorySettings,prepareMemorySettings:sdk.prepareMemorySettings,settingsOutput:output=>output.data.settings})],
    ['../host/retrieval.js',synthetic({listMemories:sdk.listMemories,browseMemories:sdk.browseMemories,
      reviewMemories:async(settings,filter)=>review(await sdk.listMemories(),settings,filter,sdk.readMemory)})],
    ['../host/storage.js',synthetic({readMemory:sdk.readMemory})],
    ['../host/mutations.js',synthetic({prepareMemory:sdk.prepareMemory,prepareRemoveMemory:sdk.prepareRemoveMemory,prepareMergeMemories:sdk.prepareMergeMemories,memoryOutput:output=>output.data.outcome})],
  ]);
  async function module(name) {
    if(modules.has(name))return modules.get(name);
    const url=new URL(name,new URL('../dev.sailry.platform/desktop/',import.meta.url)), key=url.href;
    if(modules.has(key))return modules.get(key);
    const source=await readFile(url,'utf8');
    const value=new vm.SourceTextModule(source,{context,identifier:key});modules.set(key,value);
    await value.link((name,parent)=>module(modules.has(name)?name:new URL(name,parent.identifier).href));return value;
  }
  const main=await module('./settings.js');await main.evaluate();
  const cx={notify(){},spawn(work){const job=work(cx);jobs.push(job);}};
  const view=new main.namespace.default();view.init({},cx);await jobs[0];
  async function act(work){const index=jobs.length;work();await Promise.all(jobs.slice(index));}
  return {view,cx,fields,options,prepared,completed,forgotten,act,notices,set saved(value){saved=value;},set read(value){read=value;},set complete(value){complete=value;},set browse(value){browse=value;},set catalog(value){catalog=value;}};
}

test('native selects preserve configuration guards and captured editor identity',async()=>{
  const f=await fixture();
  f.complete=async()=>({Ok:{kind:'plugin_transaction',data:{settings:{...defaults,revision:1,context_bytes:16384}}}});
  await f.act(()=>f.view.control({id:'memory-budget',value:'16'},f.cx));
  assert.equal(f.prepared[0].value.context_bytes,16384);
  assert.equal(f.prepared[0].value.revision,0);
  f.view.control({id:'memory-budget',value:'3'},f.cx);
  f.view.configuration.pending=true;
  f.view.control({id:'memory-review-age',value:'365'},f.cx);
  assert.equal(f.prepared.length,1);
  assert.equal(f.view.draft.review_after_days,90);
  f.view.configuration.pending=false;
  f.view.edit(null,f.cx);
  const owner=f.view.editing,id=`memory-kind-${f.view.dialogId}`;
  assert.equal(f.options.get(owner.title).placeholder,'Describe this memory');
  assert.equal(f.options.get(owner.body).placeholder,'Write the content');
  f.view.control({id,value:'reference'},f.cx);
  assert.equal(owner.entry.summary.kind,'reference');
  for(const field of ['loading','pending','request']) {
    owner[field]=true;f.view.control({id,value:'user'},f.cx);owner[field]=false;
    assert.equal(owner.entry.summary.kind,'reference');
  }
  f.view.dialogId++;
  f.view.control({id,value:'user'},f.cx);
  assert.equal(owner.entry.summary.kind,'reference','a stale tab event cannot edit a replacement dialog');
});

test('keeps a conflicting configuration draft until explicit reload',async()=>{
  const f=await fixture();
  f.view.draft.context_bytes=4096;
  f.saved={...defaults,revision:1,enabled:false};
  await f.act(()=>f.view.refresh(f.cx));
  assert.equal(f.view.draft.context_bytes,4096);
  assert.equal(f.view.draft.revision,0);
  f.complete=async()=>({Err:{code:'revision_conflict'}});
  await f.act(()=>f.view.saveConfiguration(f.cx));
  assert.equal(f.prepared[0].value.revision,0);
  assert.equal(f.view.configuration.error,'memory_settings_conflict');
  assert.equal(f.notices.length,1);assert.equal(f.notices[0].kind,'error');
  assert.equal(f.view.draft.context_bytes,4096);
  f.view.reloadConfiguration(f.cx);
  assert.deepEqual(plain(f.view.draft),{...defaults,revision:1,enabled:false});
});

test('retries an uncertain mutation with its original immutable request',async()=>{
  const f=await fixture();
  f.view.edit(null,f.cx);
  f.fields.set(f.view.editing.title,'Preference');f.fields.set(f.view.editing.body,'Use 中文 🙂');
  f.complete=async()=>({Err:{code:'outcome_unknown'}});
  await f.act(()=>f.view.save(f.cx));
  const owner=f.view.editing;
  assert.equal(owner.error,'memory_unknown');
  assert.equal(f.prepared.length,1);
  f.fields.set(owner.body,'Changed after admission');
  f.complete=async()=>({Ok:{kind:'plugin_transaction',data:{outcome:{kind:'memory_results'}}}});
  await f.act(()=>f.view.save(f.cx));
  assert.deepEqual(f.completed,['request-0','request-0']);
  assert.equal(f.prepared.length,1);
  assert.equal(f.prepared[0].value.value.body,'Use 中文 🙂');
  assert.equal(f.view.editing,null);
});

test('late completion from a closed editor cannot close its replacement',async()=>{
  const f=await fixture();
  f.view.edit(null,f.cx);f.fields.set(f.view.editing.title,'First');f.fields.set(f.view.editing.body,'First body');
  let resolve;
  f.complete=()=>new Promise(done=>resolve=done);
  f.view.save(f.cx);await Promise.resolve();
  f.view.close(f.cx);f.view.edit(null,f.cx);
  const replacement=f.view.editing;
  resolve({Ok:{kind:'plugin_transaction',data:{outcome:{kind:'memory_results'}}}});
  await new Promise(done=>setImmediate(done));
  assert.equal(f.view.editing,replacement);
});

test('merge keeps source revisions and editor text while storage faults remain visible',async()=>{
  const f=await fixture();
  await f.act(()=>f.view.edit(entry.summary,f.cx));
  f.read={summary:{...entry.summary,id:'22222222-2222-4222-8222-222222222222',revision:3},body:'Additional condition'};
  await f.act(()=>f.view.merge('22222222-2222-4222-8222-222222222222',f.cx));
  assert.equal(f.fields.get(f.view.editing.body),`${entry.body}\n\nAdditional condition`);
  f.complete=async()=>({Err:{code:'revision_conflict'}});
  await f.act(()=>f.view.save(f.cx));
  assert.equal(f.prepared[0].kind,'merge');
  assert.deepEqual(f.prepared[0].value.sources,[{id:'22222222-2222-4222-8222-222222222222',revision:3}]);
  assert.equal(f.view.editing.error,'memory_conflict');
  assert.ok(f.fields.get(f.view.editing.body).includes('Additional condition'));
});


test('coalesces overlapping catalog invalidations within host request capacity',async()=>{
  const f=await fixture();
  let calls=0,active=0,maximum=0,resolve;
  f.browse=async()=>{
    calls++;maximum=Math.max(maximum,++active);
    if(calls===1)await new Promise(done=>resolve=done);
    active--;return [plain(entry.summary)];
  };
  f.view.refresh(f.cx);
  f.view.refresh(f.cx);
  f.view.refresh(f.cx);
  assert.equal(calls,1);
  resolve();
  await new Promise(done=>setImmediate(done));
  assert.equal(calls,2);
  assert.equal(maximum,1);
  assert.equal(f.view.loading,false);
  assert.equal(f.view.error,null);
});

test('review uses package hints while preserving the selected search results',async()=>{
  const f=await fixture(), now=Date.now();
  const values=[{summary:{...entry.summary,updated_at_ms:now},body:'One durable fact'},
    {summary:{...entry.summary,id:'22222222-2222-4222-8222-222222222222',updated_at_ms:now-1},body:'One durable fact again'},
    {summary:{...entry.summary,id:'33333333-3333-4333-8333-333333333333',updated_at_ms:1},body:'Old verified exception'}];
  f.catalog=values.map(value=>value.summary);
  f.read=async id=>values.find(value=>value.summary.id===id);
  f.browse=async()=>[plain(values[2].summary)];
  f.view.view='review';
  await f.act(()=>f.view.refresh(f.cx));
  assert.equal(f.view.reviews.length,1);
  assert.equal(f.view.reviews[0].summary.id,values[2].summary.id);
  assert.equal(f.view.reviews[0].stale,true);
  assert.deepEqual(plain(f.view.results),[values[2].summary]);
  assert.equal(f.prepared.length,0);
  assert.equal(f.view.error,null);
});
