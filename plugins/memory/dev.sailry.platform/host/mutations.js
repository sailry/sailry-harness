import {prepareTransaction,readProjectCatalog} from 'sailry/sdk';
import {head,metadata,record,page,body,clock,indexed,scopeTag,write,remove,visible} from './storage.js';
import {capacity,clone,equivalence,identifier,next,normalized,revision,validate,limits} from './model.js';
import {terms} from './tokenize.js';
import {readMemorySettings} from './settings.js';
import {fail} from './policy.js';

async function writable(project) {
  if (project === undefined) return;
  const settings = await readMemorySettings();
  if (!settings.enabled || !settings.auto_write) fail('memory access is disabled','permission_denied');
}
function expected(current,value) {
  revision(value);
  if ((current.present ? metadata(current).summary.revision : 0) !== value) fail('memory revision changed','revision_conflict');
}
function indexedWrite(current,entry,bodyPage,bodyRevision) {
  const hash = equivalence(entry.body);
  return {kind:'index',data:{key:current.key,value:{v:1,summary:entry.summary,hash,body_page:bodyPage,body_revision:bodyRevision},
    index:{fields:[terms(entry.summary.title).join(' '),terms(entry.body).join(' ')],
      tags:[`state:${entry.summary.archived ? 'archived' : 'active'}`,scopeTag(entry.summary.project),`equivalent:${hash}`],order:entry.summary.updated_at_ms},
    expected_revision:current.revision}};
}
async function duplicate(entry,excluded) {
  if (entry.summary.archived) return;
  const candidates = await indexed({terms:[],all:['state:active',scopeTag(entry.summary.project),`equivalent:${equivalence(entry.body)}`],any:[],weights:[3,1]});
  for (const candidate of candidates) {
    const value = metadata(candidate);
    if (!excluded.has(value.summary.id) && normalized((await body(value)).body) === normalized(entry.body)) {
      fail('equivalent memory already exists; update or consolidate it','conflict');
    }
  }
}
async function projectAvailable(project) {
  if (project === null) return;
  const catalog = await readProjectCatalog();
  if (!catalog.projects.some(value=>value.id === project)) fail('memory project is unavailable','not_found');
}
function outcomeHead(current,changes) {
  Object.assign(current.value,capacity(current.value,changes));
  return write(current.record,current.value);
}
async function ownedPage(current,id) {
  if (!(id in current.value.pages)) fail('memory storage is incompatible');
  return page(id,current.value.pages[id]);
}
async function allocate(current,id) {
  let selected = Object.entries(current.value.pages).find(([,count])=>count < limits.page)?.[0];
  if (selected === undefined) {
    if (current.value.next === Number.MAX_SAFE_INTEGER) fail('memory page identifiers exhausted');
    selected = String(current.value.next++);
  }
  const stored = await page(selected,current.value.pages[selected] ?? 0);
  if (id in stored.value.bodies) fail('memory storage is incompatible');
  current.value.pages[selected] = (current.value.pages[selected] ?? 0)+1;
  return {id:selected,...stored};
}
export async function putOperations(input,expectedRevision,project) {
  const entry = clone(validate(input));
  if (entry.summary.revision !== expectedRevision) fail('invalid memory revision');
  const [current,stored,now] = await Promise.all([head(),record(entry.summary.id,project),clock()]);
  await writable(project);
  expected(stored,expectedRevision);
  const previous = stored.present ? metadata(stored) : null;
  if (previous && previous.summary.project !== entry.summary.project) fail('memory scope cannot be changed');
  if (!visible(entry.summary,project)) fail('memory belongs to another project','permission_denied');
  // Scoped writes already carry the execution project's canonical ID. Management
  // can choose another project and therefore validates against the Node catalog.
  if (project === undefined) await projectAvailable(entry.summary.project);
  const saved = next(entry,now);
  await duplicate(saved,new Set([saved.summary.id]));
  const target = previous ? {id:previous.body_page,...await ownedPage(current,previous.body_page)} : await allocate(current,saved.summary.id);
  if (previous && !(saved.summary.id in target.value.bodies)) fail('memory storage is incompatible');
  const writes = [];
  let bodyRevision = previous?.body_revision ?? saved.summary.revision;
  if (target.value.bodies[saved.summary.id]?.body !== saved.body) {
    bodyRevision = saved.summary.revision;
    target.value.bodies[saved.summary.id] = {revision:bodyRevision,body:saved.body};
    writes.push(write(target.record,target.value));
  }
  return {operations:[outcomeHead(current,[{before:previous?.summary,after:saved.summary}]),...writes,indexedWrite(stored,saved,target.id,bodyRevision)]};
}
export async function removeOperations(id,expectedRevision,project) {
  const [current,stored] = await Promise.all([head(),record(id,project)]);
  await writable(project);
  const previous = metadata(stored);
  expected(stored,expectedRevision);
  const target = await ownedPage(current,previous.body_page);
  if (!(previous.summary.id in target.value.bodies)) fail('memory storage is incompatible');
  delete target.value.bodies[previous.summary.id];
  if (--current.value.pages[previous.body_page] === 0) delete current.value.pages[previous.body_page];
  const changed = Object.keys(target.value.bodies).length ? write(target.record,target.value) : remove(target.record);
  return {operations:[outcomeHead(current,[{before:previous.summary,after:null}]),changed,remove(stored)]};
}
export async function mergeOperations(input,sources,project) {
  const entry = clone(validate(input));
  if (entry.summary.archived || !Array.isArray(sources) || !sources.length || sources.length > limits.sources) fail('invalid memory consolidation');
  const [current,target,now] = await Promise.all([head(),record(entry.summary.id,project),clock()]);
  await writable(project);
  const previous = metadata(target);
  expected(target,entry.summary.revision);
  if (previous.summary.archived) fail('memory revision changed','revision_conflict');
  if (previous.summary.project !== entry.summary.project) fail('memory scope cannot be changed');
  const excluded = new Set([entry.summary.id]), originals = [];
  for (const source of sources) {
    const id = identifier(source.id);
    if (excluded.has(id)) fail('consolidation requires distinct memories in the same scope');
    excluded.add(id);
    const stored = await record(id,project), value = metadata(stored);
    expected(stored,source.revision);
    if (value.summary.project !== entry.summary.project) fail('consolidation requires distinct memories in the same scope');
    if (value.summary.archived) fail('source memory changed','revision_conflict');
    const original = await body(value,await ownedPage(current,value.body_page));
    original.summary.archived = true;
    originals.push({stored,value,entry:next(original,now)});
  }
  const saved = next(entry,now);
  await duplicate(saved,excluded);
  const targetPage = await ownedPage(current,previous.body_page);
  if (!(saved.summary.id in targetPage.value.bodies)) fail('memory storage is incompatible');
  targetPage.value.bodies[saved.summary.id] = {revision:saved.summary.revision,body:saved.body};
  const changes = originals.map(original=>({before:original.value.summary,after:original.entry.summary}));
  changes.push({before:previous.summary,after:saved.summary});
  return {operations:[outcomeHead(current,changes),write(targetPage.record,targetPage.value),
    ...originals.map(original=>indexedWrite(original.stored,original.entry,original.value.body_page,original.value.body_revision)),
    indexedWrite(target,saved,previous.body_page,saved.summary.revision)]};
}
export async function prepareMemory(entry,expectedRevision) { return prepareTransaction((await putOperations(entry,expectedRevision)).operations); }
export async function prepareRemoveMemory(id,expectedRevision) { return prepareTransaction((await removeOperations(id,expectedRevision)).operations); }
export async function prepareMergeMemories(entry,sources) { return prepareTransaction((await mergeOperations(entry,sources)).operations); }
export function memoryOutput(output) {
  if (output?.kind !== 'plugin_transaction' || !Array.isArray(output.data)) fail('memory outcome is unavailable','outcome_unknown');
  const changed = output.data.filter(value=>value.kind === 'plugin_value' && value.data?.key.startsWith('memory/entry/'));
  if (!changed.length) fail('memory outcome is unavailable','outcome_unknown');
  return changed.every(value=>!value.data.present) ? {kind:'memory_removed',data:changed.map(value=>value.data.key.slice('memory/entry/'.length))}
    : {kind:'memory_results',data:changed.filter(value=>value.data.present).map(value=>metadata(value.data).summary)};
}
