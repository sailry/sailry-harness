// Test-only management callbacks reuse the installed package's actual private owners.
export * from './main.js';
import {prepareTransaction,completeRequest,forgetRequest,indexedValue} from 'sailry/sdk';
import {readMemorySettings,prepareMemorySettings,settingsOutput} from './settings.js';
import {listMemories,searchMemories,browseMemories} from './retrieval.js';
import {readMemory,clock,record,metadata,scopeTag} from './storage.js';
import {terms} from './tokenize.js';
import {putOperations,removeOperations,mergeOperations,memoryOutput} from './mutations.js';

async function complete(id) {
  const outcome=await completeRequest(id);
  forgetRequest(id);
  if (outcome.Err) throw Object.assign(new Error(outcome.Err.message),{code:outcome.Err.code});
  return outcome.Ok;
}
async function commit(draft) { return complete(prepareTransaction(draft.operations)); }

export async function fixture(input) {
  try {
    let value;
    switch (input.action) {
      case 'settings': value=await readMemorySettings(); break;
      case 'saveSettings': value=settingsOutput(await complete(await prepareMemorySettings(input.settings))); break;
      case 'list': value=await listMemories(input.project); break;
      case 'search': value=await searchMemories(input.project,input.query); break;
      case 'browse': value=await browseMemories(input.filter); break;
      case 'read': value=await readMemory(input.id,input.project); break;
      case 'prepare': value=await putOperations(input.entry,input.expected_revision,input.project); break;
      case 'seed':
        value=[];
        for (const entry of input.entries) {
          await commit(await putOperations(entry,0));
          value.push(await readMemory(entry.summary.id));
        }
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
