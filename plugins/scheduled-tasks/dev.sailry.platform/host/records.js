import {context, getValue, listKeys, prepareTransaction, completeRequest, forgetRequest} from "sailry/sdk";

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
function bytes(text) {
  let length = 0;
  for (const char of text) {
    const code = char.codePointAt(0);
    length += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return length;
}
export function draft(input) {
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
  const timing = input.timing;
  const first = timing?.kind === "once" ? timing.data?.at_ms : timing?.kind === "every" ? timing.data?.anchor_ms : null;
  if (!Number.isSafeInteger(first) || first < 0 || (timing.kind === "every"
      && (!Number.isSafeInteger(timing.data.interval_ms) || timing.data.interval_ms < 1000 || timing.data.interval_ms > 31536000000))) {
    throw fault("invalid_request", "Invalid task timing");
  }
  return {id, name:input.name.trim(), prompt:input.prompt, project, worktree,
    config:input.config, timing, enabled:input.enabled, queue:input.queue};
}
export function requireConfig(config) {
  if (!config) throw fault("not_configured", "Task requires an explicit model");
}
export async function checked(input) {
  const expected = revision(input.revision);
  const old = await read(input.id);
  if (old.revision !== expected) throw fault("revision_conflict", "Task changed");
  return old;
}
export async function read(id) {
  const old = await getValue(key(id));
  if (old.present) {
    if (old.value?.id !== id) throw fault("invalid_request", "Invalid stored task");
    return {...old,value:draft(old.value)};
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
export async function page(input = {}, project) {
  const page = await listKeys("task/", input.after ?? null, 32);
  const items = [];
  let size = 0, after = input.after ?? null;
  for (const name of page.keys) {
    const entry = await getValue(name);
    if (entry.present && (project === undefined || entry.value.project === project)) {
      const item = {...entry.value, revision:entry.revision};
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
