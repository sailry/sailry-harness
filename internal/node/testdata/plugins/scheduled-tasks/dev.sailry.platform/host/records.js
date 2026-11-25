import {context, getValue, listKeys, prepareHostCall, prepareTransaction, completeRequest, forgetRequest} from "sailry/sdk";

export function fault(code, message) { return Object.assign(new Error(message), {code}); }
export function key(id) {
  if (typeof id !== "string" || !/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(id)) {
    throw fault("invalid_request", "Invalid task ID");
  }
  return `task/${id}`;
}
export function revision(value) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || BigInt(value) > 9223372036854775807n) {
    throw fault("invalid_request", "Invalid task revision");
  }
  return value;
}
function ordered(value) {
  if (Array.isArray(value)) return value.map(ordered);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, ordered(value[key])]));
  }
  return value;
}
export function snapshot(value) {
  const {id, execution_node = null, name, prompt, project = null, worktree = null, config,
    execution = "team", team = null, models = {}, timing, enabled, queue} = value;
  return JSON.parse(JSON.stringify({id, execution_node, name, prompt, project, worktree, config,
    execution, team:team === null ? null : {id:team.id, revision:null}, models,
    timing:timing?.kind === "weekly" ? {...timing, data:{...timing.data,
      weekdays:[...timing.data.weekdays].sort((a,b) => a-b)}} : timing, enabled, queue}));
}
export function checkBase(input, entry) {
  const expected = revision(input.revision);
  // Source and executor revisions are independent. An explicit original value
  // proves content equality before using this Node's revision for its CAS.
  if (Object.hasOwn(input, "expected_value")) {
    if (!entry.present || JSON.stringify(ordered(snapshot(entry.value))) !== JSON.stringify(ordered(snapshot(input.expected_value)))) {
      throw fault("revision_conflict", "Task changed");
    }
  } else if (entry.revision !== expected) throw fault("revision_conflict", "Task changed");
  return entry.revision;
}
export function node(value) {
  if (!Array.isArray(value) || value.length !== 32 || !value.some(byte => byte !== 0)
      || value.some(byte => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
    throw fault("invalid_request", "Invalid execution Node");
  }
  return [...value];
}
export function sameNode(a, b) { return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((byte, index) => byte === b[index]); }
export async function hostCall(target, handler, input, readOnly = false) {
  const id = prepareHostCall(node(target), handler, input, readOnly);
  const result = await completeRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  forgetRequest(id);
  if (result.Ok.data.Err) throw fault(result.Ok.data.Err.code, result.Ok.data.Err.message);
  return result.Ok.data.Ok;
}
export function execution(input, strict = false) {
  const mode = input.execution === undefined ? "team" : input.execution;
  const team = input.team ?? null, models = input.models === undefined ? {} : input.models;
  if (!["model", "team"].includes(mode) || !models || typeof models !== "object" || Array.isArray(models)) {
    throw fault("invalid_request", "Invalid task execution");
  }
  if (team !== null && (!team || typeof team !== "object" || Array.isArray(team)
      || typeof team.id !== "string" || !/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(team.id)
      || (strict && Object.keys(team).some(key => !["id", "revision"].includes(key)))
      || (team.revision !== undefined && team.revision !== null
        && (typeof team.revision !== "string" || !/^[1-9][0-9]*$/.test(team.revision))))) {
    throw fault("invalid_request", "Invalid task team");
  }
  if (mode === "model" && (team !== null || Object.keys(models).length)) {
    throw fault("invalid_request", "Model execution cannot select a team or member models");
  }
  // A recurring task chooses an identity; each admitted run freezes its current revision.
  return {execution:mode, team:team === null ? null : {id:team.id, revision:null}, models};
}
function bytes(text) {
  let length = 0;
  for (const char of text) {
    const code = char.codePointAt(0);
    length += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return length;
}
export function timing(input, strict = false) {
  const fields = {once:["at_ms"],every:["anchor_ms","interval_ms"],weekly:["timezone","weekdays","hour","minute"]};
  if (!input || typeof input !== "object" || Array.isArray(input) || !Object.hasOwn(fields,input.kind)
      || !input.data || typeof input.data !== "object" || Array.isArray(input.data)) {
    throw fault("invalid_request", "Invalid task timing");
  }
  if ((strict || input.kind === "weekly") && (Object.keys(input).some(key => !["kind","data"].includes(key))
      || Object.keys(input.data).some(key => !fields[input.kind].includes(key)))) {
    throw fault("invalid_request", "Invalid task timing");
  }
  if (input.kind === "weekly") {
    const {timezone,weekdays,hour,minute} = input.data;
    if (typeof timezone !== "string" || !timezone || timezone !== timezone.trim()
        || !Array.isArray(weekdays) || !weekdays.length
        || weekdays.some(day => !Number.isInteger(day) || day < 1 || day > 7)
        || new Set(weekdays).size !== weekdays.length
        || !Number.isInteger(hour) || hour < 0 || hour > 23
        || !Number.isInteger(minute) || minute < 0 || minute > 59) {
      throw fault("invalid_request", "Invalid task timing");
    }
    return {kind:"weekly",data:{timezone,weekdays:[...weekdays].sort((left,right) => left-right),hour,minute}};
  }
  const first = input.kind === "once" ? input.data.at_ms : input.data.anchor_ms;
  if (!Number.isSafeInteger(first) || first < 0 || (input.kind === "every"
      && (!Number.isSafeInteger(input.data.interval_ms) || input.data.interval_ms < 1))) {
    throw fault("invalid_request", "Invalid task timing");
  }
  return input;
}
export function draft(input, capture = true) {
  const id = input.id ?? context().invocation;
  key(id);
  if (typeof input.name !== "string" || !input.name.trim() || bytes(input.name) > 256
      || typeof input.prompt !== "string" || !input.prompt.trim() || bytes(input.prompt) > 32768
      || typeof input.enabled !== "boolean" || typeof input.queue !== "string"
      || !/^[a-zA-Z0-9._-]{1,128}$/.test(input.queue)) {
    throw fault("invalid_request", "Invalid task content or queue");
  }
  const project = input.project ?? null;
  const worktree = input.worktree ?? null;
  requireConfig(input.config);
  if (!project && worktree) throw fault("invalid_request", "Task worktree requires a project");
  if (input.config?.assistant || input.config?.resource) {
    throw fault("invalid_request", "Task requires a project or unassigned scope");
  }
  const schedule = timing(input.timing);
  const executionNode = input.execution_node == null ? capture ? node(context().execution_node) : null : node(input.execution_node);
  return {id, execution_node:executionNode, name:input.name.trim(), prompt:input.prompt, project, worktree,
    config:input.config, ...execution(input), timing:schedule, enabled:input.enabled, queue:input.queue};
}
export function requireConfig(config) {
  if (!config) throw fault("not_configured", "Task requires an explicit model");
}
export async function checked(input) {
  const old = await read(input.id);
  checkBase(input, old);
  return old;
}
export async function read(id) {
  const old = await getValue(key(id));
  if (old.present) {
    if (old.value?.id !== id) throw fault("invalid_request", "Invalid stored task");
    return {...old,value:draft(old.value, false)};
  }
  return old;
}
export async function commit(operations) {
  const id = prepareTransaction(operations);
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data;
}
// Personal copies discover records; mutations use the execution Node's current
// value and revision. An offline copy stays visible without an editable revision.
export async function visible(id) {
  const entry = await getValue(key(id));
  if (!entry.present) return null;
  const item = {...entry.value, revision:entry.revision};
  const target = item.execution_node;
  if (target == null || sameNode(target, context().execution_node)) return item;
  try { return await hostCall(target, "record", {id}, true); }
  catch (error) {
    if (["permission_denied", "not_found"].includes(error.code)) return null;
    if (!["unavailable", "busy", "outcome_unknown", "internal", "cancelled"].includes(error.code)) throw error;
    return {...item, revision:null, unavailable:true};
  }
}
export async function page(input = {}, project) {
  const page = await listKeys("task/", input.after ?? null, 32);
  const items = [];
  let size = 0, after = input.after ?? null;
  const entries=await Promise.allSettled(page.keys.map(name=>visible(name.slice(5))));
  for (const [index,name] of page.keys.entries()) {
    const result=entries[index];if(result.status==='rejected')throw result.reason;
    const item = result.value;
    if (item && (project === undefined || item.project === project)) {
      const length = bytes(JSON.stringify(item));
      // Keep task pages within the host callback result budget.
      if (items.length && size + length > 128 * 1024) return {items, after};
      items.push(item); size += length;
    }
    after = name;
  }
  return {items, after:page.after};
}
export async function result(action) {
  try { return {Ok:await action()}; }
  catch (error) { return {Err:{code:error.code ?? "invalid_request", message:error.message}}; }
}
