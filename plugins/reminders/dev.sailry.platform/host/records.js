import { context, getValue, listKeys, prepareTransaction, completeRequest, forgetRequest } from "sailry/sdk";

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
export function bytes(text) {
  let length = 0;
  for (const char of text) {
    const code = char.codePointAt(0);
    length += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return length;
}
export function draft(input) {
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
  return {id, project:input.project, title:input.title.trim(), message:input.message, completed:input.completed, due_ms:input.due_ms};
}
export function stored(value, id) {
  try {
    const item = draft(value);
    if (item.id !== id || item.title !== value.title
        || (value.notified_ms !== null && (!Number.isSafeInteger(value.notified_ms) || value.notified_ms < 0))) {
      throw new Error("invalid record");
    }
    return {...item, notified_ms:value.notified_ms};
  } catch (_) { throw fault("invalid_request", "Reminder storage is incompatible"); }
}
export async function commit(operations) {
  const id = prepareTransaction(operations);
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data;
}
export async function page(input = {}, scope) {
  if (scope !== undefined) project(scope);
  let after = input.after ?? null;
  if (after !== null) key(typeof after === "string" && after.startsWith("reminder/") ? after.slice(9) : "");
  const items = [];
  let scanned = 0, size = 0;
  do {
    const batch = await listKeys("reminder/", after, 32);
    for (const name of batch.keys) {
      const id = name.slice(9), entry = await read(id);
      if (entry.present && (scope === undefined || entry.value.project === scope)) {
        const item = {...stored(entry.value, id), revision:entry.revision};
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
