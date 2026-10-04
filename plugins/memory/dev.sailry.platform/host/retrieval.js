import {getValue} from 'sailry/sdk';
import {indexed,metadata,body,page,entryKey,scopeTag,head} from './storage.js';
import {limits,ordered} from './model.js';
import {query as tokenize} from './tokenize.js';
import {bytes,fail} from './policy.js';
import {review} from './review.js';

async function find({project,all_projects=false,archived=false,query,recall=false}) {
  if (typeof query !== 'string' || bytes(query) > limits.query) fail('memory query is too long');
  const all = [`state:${archived ? 'archived' : 'active'}`];
  const any = all_projects ? [] : recall ? Array.from(new Set([scopeTag(null),scopeTag(project)])) : [scopeTag(project)];
  if (/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(query.trim())) {
    const current = await getValue(entryKey(query.trim()));
    if (!current.present) return [];
    const value = metadata(current), owner = value.summary.project;
    return value.summary.archived === archived && (all_projects || owner === project || (recall && owner === null)) ? [current] : [];
  }
  const terms = tokenize(query);
  if (query.trim() && !terms.length) return [];
  return indexed({terms,weights:[3,1],all,any},recall ? 20 : Infinity);
}
async function catalog(project) {
  const before = await head();
  const records = await indexed({terms:[],all:[],any:project === undefined ? [] : Array.from(new Set([scopeTag(null),scopeTag(project)])),weights:[3,1]});
  const after = await head();
  if (before.record.revision !== after.record.revision) fail('memory catalog changed','revision_conflict');
  return {revision:after.record.revision,entries:records.map(metadata)};
}
export async function listMemories(project) {
  return (await catalog(project)).entries.map(entry=>entry.summary).sort(ordered);
}
export async function reviewMemories(settings,filter) {
  const captured = await catalog(filter.all_projects ? undefined : filter.project);
  const entries = new Map(captured.entries.map(entry=>[entry.summary.id,entry])), pages = new Map();
  const hints = await review(captured.entries.map(entry=>entry.summary),settings,filter,async id=>{
    const entry = entries.get(id);
    if (!pages.has(entry.body_page)) pages.set(entry.body_page,page(entry.body_page));
    return body(entry,await pages.get(entry.body_page));
  });
  if (captured.revision !== (await head()).record.revision) fail('memory catalog changed during review','revision_conflict');
  return hints;
}
export async function browseMemories(filter) {
  return (await find(filter)).map(record=>metadata(record).summary);
}
export async function searchMemories(project,query) {
  const results = [];
  for (const current of await find({project,query,recall:true})) results.push(await body(metadata(current)));
  return results;
}
