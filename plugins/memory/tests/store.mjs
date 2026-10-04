import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {equivalence} from '../dev.sailry.platform/host/model.js';
import {terms} from '../dev.sailry.platform/host/tokenize.js';

const clone = value => JSON.parse(JSON.stringify(value));
export const id = index => `11111111-1111-4111-8111-${String(index).padStart(12,'0')}`;
export const project = '22222222-2222-4222-8222-222222222222';
export const entry = (index,body,changes={}) => ({summary:{id:id(index),project,title:'Verified constraint',kind:'project',
  revision:0,updated_at_ms:0,archived:false,...changes},body});
export const fault = (code,message) => Object.assign(new Error(message),{code});

export async function fixture() {
  const records = new Map(), indexes = new Map(), drafts = new Map(), receipts = new Map();
  const prepared = [], completed = [], forgotten = [], reads = [], searches = [], catalogReads = [];
  let now = 100*86_400_000, state = null, onRead = null;
  const get = key => clone(records.get(key) ?? {key,revision:'0',value:null,present:false});
  function store(key,value,revision='1',index) {
    records.set(key,{key,value:clone(value),revision,present:true});
    if (index) indexes.set(key,clone(index)); else indexes.delete(key);
  }
  function configure(value) {
    const settings={...value};delete settings.revision;
    store('memory/settings',{v:1,settings},String(value.revision ?? 1));
  }
  function seed(entries) {
    for (const key of [...records.keys()]) if (key.startsWith('memory/entry/') || key.startsWith('memory/body/') || key === 'memory/catalog') {
      records.delete(key);indexes.delete(key);
    }
    const pages = {}, bodies = new Map();
    for (const [index,original] of entries.entries()) {
      const value=clone(original), page=String(Math.floor(index/2));
      value.summary.revision ||= 1;
      pages[page]=(pages[page] ?? 0)+1;
      if (!bodies.has(page)) bodies.set(page,{});
      bodies.get(page)[value.summary.id]={body:value.body,revision:value.summary.revision};
      store(`memory/entry/${value.summary.id}`,{v:1,summary:value.summary,hash:equivalence(value.body),body_page:page,body_revision:value.summary.revision},String(value.summary.revision),
        {fields:[terms(value.summary.title).join(' '),terms(value.body).join(' ')],
          tags:[`state:${value.summary.archived ? 'archived' : 'active'}`,`scope:${value.summary.project ?? 'global'}`,`equivalent:${equivalence(value.body)}`],order:value.summary.updated_at_ms});
    }
    for (const [page,value] of bodies) store(`memory/body/${page}`,{v:1,bodies:value});
    store('memory/catalog',{v:1,pages,active:entries.filter(entry=>!entry.summary.archived).length,
      archived:entries.filter(entry=>entry.summary.archived).length,next:bodies.size});
  }
  function search(query) {
    const weights=query.weights ?? [1,1];
    const selected=[...indexes].filter(([,index])=>(query.all ?? []).every(tag=>index.tags.includes(tag))
      && (!(query.any ?? []).length || query.any.some(tag=>index.tags.includes(tag))))
      .map(([key,index])=>{
        const score=(query.terms ?? []).reduce((score,term)=>score+index.fields.reduce((sum,text,column)=>
          sum+text.split(' ').filter(value=>value===term).length*weights[column],0),0);
        return {key,index,score};
      }).filter(value=>!(query.terms ?? []).length || value.score > 0)
      .sort((left,right)=>right.score-left.score || right.index.order-left.index.order || left.key.localeCompare(right.key));
    const offset=query.offset ?? 0, limit=query.limit ?? 100, page=selected.slice(offset,offset+limit);
    return {entries:page.map(value=>get(value.key)),next:offset+page.length < selected.length ? offset+page.length : null,now_ms:now};
  }
  function apply(operations) {
    const original=clone([...records]), originalIndexes=clone([...indexes]), output=[];
    try {
      for (const operation of operations) {
        const {key,expected_revision}=operation.data, previous=get(key);
        if (String(expected_revision) !== previous.revision) throw fault('revision_conflict','plugin value changed');
        const saved={key,revision:String(BigInt(previous.revision)+1n),present:operation.kind !== 'remove',
          value:operation.kind === 'remove' ? null : clone(operation.data.value)};
        records.set(key,saved);
        if (operation.kind === 'index') indexes.set(key,clone(operation.data.index)); else indexes.delete(key);
        output.push({kind:'plugin_value',data:clone(saved)});
      }
      return {kind:'plugin_transaction',data:output};
    } catch (error) {
      records.clear();indexes.clear();
      for (const [key,value] of original) records.set(key,value);
      for (const [key,value] of originalIndexes) indexes.set(key,value);
      throw error;
    }
  }
  const sdk={getValue:async key=>{reads.push(key);await onRead?.(key);return get(key);},
    searchValues:async query=>{searches.push(clone(query));return search(query);},
    readProjectCatalog:async()=>{catalogReads.push(true);return {projects:[{id:project,name:'Current'},{id:id(9000),name:'Foreign'}],worktrees:[]};},
    prepareTransaction:operations=>{const id=`request-${prepared.length}`;const frozen=clone(operations);drafts.set(id,frozen);prepared.push({id,operations:frozen});return id;},
    completeRequest:async id=>{completed.push(id);if (!receipts.has(id)) {
      try {receipts.set(id,{Ok:apply(drafts.get(id))});} catch (error) {receipts.set(id,{Err:{code:error.code,message:error.message}});}
    }return clone(receipts.get(id));},forgetRequest:id=>{forgotten.push(id);drafts.delete(id);},newId:()=>id(9999),
    readTurnState:()=>clone(state),stageTurnState:value=>{state=clone(value);}};
  const context=vm.createContext({});
  const bridge=new vm.SyntheticModule(Object.keys(sdk),function(){for (const [key,value] of Object.entries(sdk))this.setExport(key,value);},{context});
  const modules=new Map(), pendingModules=new Map();
  async function module(name) {
    if (name === 'sailry/sdk') return bridge;
    const url=new URL(name,new URL('../dev.sailry.platform/host/',import.meta.url)), key=url.href;
    if (modules.has(key)) return modules.get(key);
    if (!pendingModules.has(key)) pendingModules.set(key,(async()=>{
      const source=await readFile(url,'utf8'), value=new vm.SourceTextModule(source,{context,identifier:key});
      modules.set(key,value);return value;
    })());
    return pendingModules.get(key);
  }
  const main=await module('main.js');
  await main.link((name,parent)=>module(name === 'sailry/sdk' ? name : new URL(name,parent.identifier).href));
  await main.evaluate();
  const namespace=name=>modules.get(new URL(name,new URL('../dev.sailry.platform/host/',import.meta.url)).href).namespace;
  return {records,indexes,sdk,prepared,completed,forgotten,reads,searches,catalogReads,get,store,configure,seed,apply,search,
    main:main.namespace,mutations:namespace('mutations.js'),storage:namespace('storage.js'),settings:namespace('settings.js'),retrieval:namespace('retrieval.js'),
    get state(){return clone(state);},set state(value){state=clone(value);},set now(value){now=value;},set onRead(value){onRead=value;}};
}
