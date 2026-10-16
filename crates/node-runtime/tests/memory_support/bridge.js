// Test-only management callbacks reuse the installed package's actual private owners.
export * from './main.js';
import {prepareTransaction,completeRequest,forgetRequest,indexedValue} from 'sailry/sdk';
import {readMemorySettings,prepareMemorySettings,outcome} from './settings.js';
import {listMemories,searchMemories,browseMemories} from './retrieval.js';
import {readMemory,clock,record,metadata,scopeTag,head,page,write,bodyKey} from './storage.js';
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

async function seedBatch(entries) {
  if (!Array.isArray(entries) || entries.length < 1 || entries.length > 32
    || entries.some(entry=>entry.summary.project !== null || entry.summary.revision !== 0)
    || new Set(entries.map(entry=>entry.summary.id)).size !== entries.length) {
    throw Object.assign(new Error('invalid memory seed batch'),{code:'invalid_request'});
  }
  const current=await head(), now=await clock();
  const saved=entries.map(entry=>next(entry,now));
  const records=await Promise.all(saved.map(entry=>record(entry.summary.id)));
  if (records.some(record=>record.present)) throw new Error('memory seed entry already exists');
  const pages=new Map(), indexes=[];
  Object.assign(current.value,capacity(current.value,saved.map(after=>({after:after.summary}))));
  for (const [index,entry] of saved.entries()) {
    let selected=Object.entries(current.value.pages).find(([,count])=>count < limits.page)?.[0];
    if (selected === undefined) selected=String(current.value.next++);
    if (!pages.has(selected)) pages.set(selected,await page(selected,current.value.pages[selected] ?? 0));
    const target=pages.get(selected);
    target.value.bodies[entry.summary.id]={revision:entry.summary.revision,body:entry.body};
    current.value.pages[selected]=(current.value.pages[selected] ?? 0)+1;
    indexes.push({kind:'index',data:{key:records[index].key,
      value:{v:1,summary:entry.summary,hash:equivalence(entry.body),body_page:selected,body_revision:entry.summary.revision},
      index:{fields:[entry.summary.title,terms(entry.body).join(' ')],
        source:{key:bodyKey(selected),pointer:`/bodies/${entry.summary.id.replace(/~/g,'~0').replace(/\//g,'~1')}/body`},
        tags:[`state:${entry.summary.archived ? 'archived' : 'active'}`,scopeTag(entry.summary.project),`equivalent:${equivalence(entry.body)}`],order:entry.summary.updated_at_ms},
      expected_revision:records[index].revision}});
  }
  await commit({operations:[write(current.record,catalog(current.value)),
    ...Array.from(pages.values(),target=>write(target.record,target.value)),...indexes]});
  return saved;
}

async function seed(entries) {
  if (!Array.isArray(entries) || entries.length < 1 || entries.length > 256) {
    throw Object.assign(new Error('invalid memory seed batch'),{code:'invalid_request'});
  }
  const saved=[];
  for (let offset=0;offset < entries.length;offset+=32) {
    saved.push(...await seedBatch(entries.slice(offset,offset+32)));
  }
  return saved;
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
      case 'seed':
        value=await seed(input.entries);
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
