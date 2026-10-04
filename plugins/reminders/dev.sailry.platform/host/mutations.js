import {readProjectCatalog} from "sailry/sdk";
import {key, read, revision, draft, stored, fault} from "./records.js";
import {changes, removal} from "./schedules.js";

function owned(item, project) {
  if (project !== undefined && item.project !== project) throw fault("permission_denied", "Reminder belongs to another project");
}
export async function saveOperations(input, project) {
  const item = draft(input), expected = revision(input.revision), old = await read(item.id);
  owned(item, project);
  if (old.present) owned(old.value, project);
  if (old.revision !== expected) throw fault("revision_conflict", "Reminder changed");
  // Tool writes already carry the execution project's canonical ID. Management
  // chooses an association explicitly and checks the execution Node's catalog.
  if (project === undefined && item.project !== null) {
    const catalog = await readProjectCatalog();
    if (!catalog.projects.some(project => project.id === item.project)) throw fault("not_found", "Reminder project is unavailable");
  }
  item.notified_ms = old.present && old.value.due_ms === item.due_ms ? old.value.notified_ms : null;
  return {item, operations:[{kind:"write",data:{key:key(item.id),value:item,expected_revision:expected}},
    ...await changes(item, String(BigInt(expected) + 1n))]};
}
export async function removeOperations(input, project) {
  const expected = revision(input.revision), old = await read(input.id);
  if (!old.present) throw fault("not_found", "Reminder is unavailable");
  owned(old.value, project);
  if (old.revision !== expected) throw fault("revision_conflict", "Reminder changed");
  return {item:old.value, operations:[{kind:"remove",data:{key:key(input.id),expected_revision:expected}},
    ...await removal(input.id)]};
}
export function transactionResult(output, id, removed) {
  const entry = output?.kind === "plugin_transaction" && Array.isArray(output.data)
    ? output.data.find(item => item.kind === "plugin_value" && item.data?.key === key(id))?.data : null;
  if (!entry || entry.present !== !removed) throw fault("outcome_unknown", "Reminder outcome is unavailable");
  revision(entry.revision);
  return removed ? {id, revision:entry.revision, removed:true}
    : {...stored(entry.value, id), revision:entry.revision};
}
