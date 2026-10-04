import {readTurnState,stageTurnState,newId} from "sailry/sdk";
import {bytes,index,retrieve,values,reads,requireRead,fail,argumentsFor} from "./policy.js";
import {readMemorySettings} from './settings.js';
import {listMemories,searchMemories,reviewMemories} from './retrieval.js';
import {readMemory} from './storage.js';
import {putOperations,removeOperations,mergeOperations,memoryOutput} from './mutations.js';

export async function initialize({tools,project,resources}) {
  const settings = await readMemorySettings();
  const state = {settings,project,remaining:settings.context_bytes,reads:[]};
  let instruction = "";
  if (settings.enabled && resources) {
    instruction = index(await listMemories(project), project, settings);
    state.remaining = Math.max(0,state.remaining-bytes(instruction));
  }
  stageTurnState(state);
  return {instruction,tools:tools.filter(name => settings.enabled && (settings.auto_write || ["search_memory","review_memories"].includes(name)))};
}

export function beforeModel({results}) {
  const state = readTurnState();
  state.reads = reads(results);
  stageTurnState(state);
  return null;
}

async function enabled(write) {
  const state = readTurnState();
  const current = await readMemorySettings();
  if (!state.settings.enabled || !current.enabled || (write && (!state.settings.auto_write || !current.auto_write))) fail("memory access is disabled","permission_denied");
  return state;
}
const protect = handler => async input => {
  try { return await handler(input); }
  catch (error) { return {error:{code:error.code ?? "internal",message:error.message},isError:true}; }
};

export const search = protect(async input => {
  const value = argumentsFor(input,["query"],["query"]);
  const state = await enabled(false);
  const entries = retrieve(await searchMemories(state.project,value.query),state);
  stageTurnState(state);
  return {entries,budget_remaining_bytes:state.remaining,budget_limited:true};
});
export const review = protect(async input => {
  argumentsFor(input,[],[]);
  const state = await enabled(false);
  const hints = values(await reviewMemories(await readMemorySettings(),
    {project:state.project,all_projects:false}),state);
  stageTurnState(state);
  return {candidates:hints};
});
export const save = protect(async input => {
  const value = argumentsFor(input,["id","expected_revision","global","archived","kind","title","body"],["expected_revision","kind","title","body"]);
  const state = await enabled(true);
  let project = value.global ? null : state.project;
  if (value.id != null) {
    project = (await readMemory(value.id,state.project)).summary.project;
    requireRead(state,value.id,value.expected_revision);
  }
  if (project === null && ((!value.global && value.id == null) || !["user","feedback"].includes(value.kind))) fail("global memory requires an explicit cross-project preference or correction; project facts require a project");
  return putOperations({summary:{id:value.id ?? newId(),project,title:value.title,kind:value.kind,revision:value.expected_revision,updated_at_ms:0,archived:value.archived ?? false},body:value.body},value.expected_revision,state.project);
});
export const forget = protect(async input => {
  const value = argumentsFor(input,["id","expected_revision"],["id","expected_revision"]);
  const state = await enabled(true);
  await readMemory(value.id,state.project);
  return removeOperations(value.id,value.expected_revision,state.project);
});
export const consolidate = protect(async input => {
  const value = argumentsFor(input,["id","expected_revision","title","body","sources"],["id","expected_revision","title","body","sources"]);
  const state = await enabled(true);
  if (!Array.isArray(value.sources) || value.sources.length < 1 || value.sources.length > 16) fail("invalid consolidation arguments");
  const entry = await readMemory(value.id,state.project);
  requireRead(state,value.id,value.expected_revision);
  for (const source of value.sources) {
    if (!source || Object.keys(source).some(key => !["id","revision"].includes(key)) || typeof source.id !== "string" || !Number.isSafeInteger(source.revision) || source.revision < 1) fail("invalid consolidation arguments");
    await readMemory(source.id,state.project);
    requireRead(state,source.id,source.revision);
  }
  entry.summary.revision = value.expected_revision;
  entry.summary.title = value.title;
  entry.body = value.body;
  return mergeOperations(entry,value.sources,state.project);
});
export function result({output}) {
  if (output.isError) return output;
  const outcome = memoryOutput(output);
  return outcome.kind === 'memory_removed' ? {removed:outcome.data} : {updated:outcome.data};
}
