import { context, getValue, listKeys, prepareHostCall, prepareTransaction, completeRequest, forgetRequest } from "sailry/sdk";

export function fault(code, message) { return Object.assign(new Error(message), {code}); }
export function key(id) {
  if (typeof id !== "string" || !/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(id)) {
    throw fault("invalid_request", "Invalid reminder ID");
  }
  return `reminder/${id}`;
}
export function project(value) {
  if (value !== null) key(value);
  return value;
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
  const outcome = await completeRequest(id);
  if (outcome.Err) throw fault(outcome.Err.code, outcome.Err.message);
  forgetRequest(id);
  const result = outcome.Ok.data;
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok;
}
export async function read(id) {
  const entry = await getValue(key(id));
  if (entry.present) stored(entry.value, id);
  return entry;
}
export function revision(value) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/.test(value) || BigInt(value) > 9223372036854775807n) {
    throw fault("invalid_request", "Invalid reminder revision");
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
  const {id, execution_node = null, project, title, message, completed, due_ms, notified_ms = null} = value;
  return JSON.parse(JSON.stringify({id, execution_node, project, title, message, completed, due_ms, notified_ms}));
}
export function checkBase(input, entry) {
  const expected = revision(input.revision);
  // Source and executor revisions are independent. An explicit original value
  // proves content equality before using this Node's revision for its CAS.
  if (Object.hasOwn(input, "expected_value")) {
    if (!entry.present || JSON.stringify(ordered(snapshot(entry.value))) !== JSON.stringify(ordered(snapshot(input.expected_value)))) {
      throw fault("revision_conflict", "Reminder changed");
    }
  } else if (entry.revision !== expected) throw fault("revision_conflict", "Reminder changed");
  return entry.revision;
}
export function bytes(text) {
  let length = 0;
  for (const char of text) {
    const code = char.codePointAt(0);
    length += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return length;
}
export function draft(input, capture = true) {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw fault("invalid_request", "Invalid reminder");
  const id = input.id ?? context().invocation;
  key(id);
  project(input.project);
  if (typeof input.title !== "string" || !input.title.trim() || bytes(input.title) > 512
      || typeof input.message !== "string" || bytes(input.message) > 4096
      || typeof input.completed !== "boolean"
      || (input.due_ms !== null && (!Number.isSafeInteger(input.due_ms) || input.due_ms < 0))) {
    throw fault("invalid_request", "Invalid reminder content or time");
  }
  const execution = input.execution_node == null ? capture ? node(context().execution_node) : null : node(input.execution_node);
  return {id, execution_node:execution, project:input.project,
    title:input.title.trim(), message:input.message, completed:input.completed, due_ms:input.due_ms};
}
export function stored(value, id) {
  try {
    const item = draft(value, false);
    if (item.id !== id || item.title !== value.title
        || (value.notified_ms !== null && (!Number.isSafeInteger(value.notified_ms) || value.notified_ms < 0))) {
      throw new Error("invalid record");
    }
    return {...item, execution_node:value.execution_node == null ? null : node(value.execution_node), notified_ms:value.notified_ms};
  } catch (_) { throw fault("invalid_request", "Reminder storage is incompatible"); }
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
  const entry = await read(id);
  if (!entry.present) return null;
  const item = {...stored(entry.value, id), revision:entry.revision};
  const target = item.execution_node;
  if (target == null || sameNode(target, context().execution_node)) return item;
  try { return await hostCall(target, "record", {id}, true); }
  catch (error) {
    if (["permission_denied", "not_found"].includes(error.code)) return null;
    if (!["unavailable", "busy", "outcome_unknown", "internal", "cancelled"].includes(error.code)) throw error;
    return {...item, revision:null, unavailable:true};
  }
}
export async function page(input = {}, scope) {
  if (scope !== undefined) project(scope);
  if (input.completed !== undefined && typeof input.completed !== "boolean") {
    throw fault("invalid_request", "Invalid reminder completion filter");
  }
  let after = input.after ?? null;
  if (after !== null) key(typeof after === "string" && after.startsWith("reminder/") ? after.slice(9) : "");
  const items = [];
  let scanned = 0, size = 0;
  do {
    const batch = await listKeys("reminder/", after, 32);
    const entries=await Promise.allSettled(batch.keys.map(name=>visible(name.slice(9))));
    for (const [index,name] of batch.keys.entries()) {
      const result=entries[index];if(result.status==='rejected')throw result.reason;
      const item = result.value;
      if (item && (scope === undefined || item.project === scope)
          && (input.completed === undefined || item.completed === input.completed)) {
        const length = bytes(JSON.stringify(item));
        if (items.length && size + length > 128 * 1024) return {items, after};
        items.push(item); size += length;
      }
      after = name; scanned++;
      if (items.length === 32 || scanned === 128) {
        return {items, after:name === batch.keys.at(-1) && batch.after === null ? null : after};
      }
    }
    after = batch.after;
  } while (after !== null);
  return {items, after:null};
}
export async function result(action) {
  try { return {Ok:await action()}; }
  catch (error) { return {Err:{code:error.code ?? "invalid_request", message:error.message}}; }
}
