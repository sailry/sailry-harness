// Test-only management callbacks reuse the installed package's actual private owners.
export * from './main.js';
import {prepareTransaction,completeRequest,forgetRequest,indexedValue} from 'sailry/sdk';
import {readMemorySettings,prepareMemorySettings,outcome} from './settings.js';
import {listMemories,searchMemories,browseMemories} from './retrieval.js';
import {readMemory,clock,record,metadata,scopeTag,bodyKey,entryKey,headKey} from './storage.js';
import {capacity,catalog,equivalence,next,limits} from './model.js';
import {terms} from './tokenize.js';
import {putOperations,removeOperations,mergeOperations,memoryOutput} from './mutations.js';

async function complete(id) {
  const outcome=await completeRequest(id);
  forgetRequest(id);
  if (outcome.Err) throw Object.assign(new Error(outcome.Err.message),{code:outcome.Err.code});
  return outcome.Ok;
}
async function commit(draft) { return complete(prepareTransaction(draft.operations)); }

// Produce initial durable rows using the installed package's real validation and indexing owners.
// Rust writes these only after shutdown; all capacity mutations still use the public SDK.
async function seedPlan(entries, previous) {
  if (!Array.isArray(entries) || entries.length < 1 || entries.length > 256
    || entries.some(entry=>entry.summary.project !== null || entry.summary.revision !== 0)
    || new Set(entries.map(entry=>entry.summary.id)).size !== entries.length) {
    throw Object.assign(new Error('invalid memory seed batch'),{code:'invalid_request'});
  }
  const current=catalog(previous ?? null), now=await clock();
  const saved=entries.map(entry=>next(entry,now)), pages=new Map(), rows=[];
  Object.assign(current,capacity(current,saved.map(after=>({after:after.summary}))));
  for (const entry of saved) {
    let selected=Object.entries(current.pages).find(([,count])=>count < limits.page)?.[0];
    if (selected === undefined) selected=String(current.next++);
    if (!pages.has(selected)) {
      if (current.pages[selected]) throw new Error('memory seed page must be new');
      pages.set(selected,{v:1,bodies:{}});
    }
    pages.get(selected).bodies[entry.summary.id]={revision:entry.summary.revision,body:entry.body};
    current.pages[selected]=(current.pages[selected] ?? 0)+1;
    rows.push({key:entryKey(entry.summary.id),
      value:{v:1,summary:entry.summary,hash:equivalence(entry.body),body_page:selected,body_revision:entry.summary.revision},
      index:{fields:[entry.summary.title,terms(entry.body).join(' ')],
        source:{key:bodyKey(selected),pointer:`/bodies/${entry.summary.id.replace(/~/g,'~0').replace(/\//g,'~1')}/body`},
        tags:[`state:${entry.summary.archived ? 'archived' : 'active'}`,scopeTag(entry.summary.project),`equivalent:${equivalence(entry.body)}`],order:entry.summary.updated_at_ms}});
  }
  rows.push(...Array.from(pages,([page,value])=>({key:bodyKey(page),value,index:null})));
  return {saved,rows,catalog:catalog(current),head:headKey};
}

export async function fixture(input) {
  try {
    let value;
    switch (input.action) {
      case 'settings': value=await readMemorySettings(); break;
      case 'saveSettings': value=outcome(await complete(await prepareMemorySettings(input.settings))); break;
      case 'list': value=await listMemories(input.project); break;
      case 'search': value=await searchMemories(input.project,input.query); break;
      case 'browse': value=await browseMemories(input.filter); break;
      case 'read': value=await readMemory(input.id,input.project); break;
      case 'prepare': value=await putOperations(input.entry,input.expected_revision,input.project); break;
      case 'seedPlan':
        value=await seedPlan(input.entries,input.catalog);
        break;
      case 'put':
        await commit(await putOperations(input.entry,input.expected_revision,input.project));
        value=await readMemory(input.entry.summary.id,input.project);
        break;
      case 'remove': value=memoryOutput(await commit(await removeOperations(input.id,input.expected_revision,input.project))).data; break;
      case 'merge':
        await commit(await mergeOperations(input.entry,input.sources,input.project));
        value=await readMemory(input.entry.summary.id,input.project);
        break;
      case 'clock': value=await clock(); break;
      case 'age': {
        const stored=await record(input.id), entry=await readMemory(input.id), item=metadata(stored);
        item.summary.updated_at_ms=input.updated_at_ms;
        await complete(indexedValue(stored.key,item,{fields:[terms(entry.summary.title).join(' '),terms(entry.body).join(' ')],
          tags:[`state:${entry.summary.archived ? 'archived' : 'active'}`,scopeTag(entry.summary.project),`equivalent:${item.hash}`],order:input.updated_at_ms},stored.revision));
        value=await readMemory(input.id);
        break;
      }
      default: throw Object.assign(new Error('unknown memory fixture action'),{code:'invalid_request'});
    }
    return {Ok:value};
  } catch (error) { return {Err:{code:error.code ?? 'internal',message:error.message}}; }
}
