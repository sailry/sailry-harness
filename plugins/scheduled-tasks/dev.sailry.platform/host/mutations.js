import {readProjectCatalog} from "sailry/sdk";
import {key, read, revision, draft, checked, page, fault} from "./records.js";
import {query, changes, removal, configuration} from "./dispatch.js";

function owned(item, project) {
  if (project !== undefined && item.project !== project) throw fault("permission_denied", "Task belongs to another project");
}
export async function listing(input, project) {
  const records = await page(input ?? {}, project), schedules = await query("list_schedules");
  return {...records,items:records.items.map(item => ({...item,next_ms:schedules.find(schedule => schedule.id === item.id)?.next_ms ?? null}))};
}
export async function saveOperations(input, project) {
  const item = draft(input), old = await checked({...input,id:item.id});
  owned(item, project);
  if (old.present) owned(old.value, project);
  item.config = await configuration(item.config);
  // Management has a Node-wide catalog; tools retain their admitted resource scope.
  // The Node validates resources again when admitting the callback's session.
  if (item.project && project === undefined) {
    const catalog = await readProjectCatalog();
    if (!catalog.projects.some(project => project.id === item.project)
        || (item.worktree && !catalog.worktrees.some(tree => tree.id === item.worktree && tree.project === item.project))) {
      throw fault("not_found", "Task project or worktree is unavailable");
    }
  }
  return {item,operations:[{kind:"write",data:{key:key(item.id),value:item,expected_revision:old.revision}},...await changes(item)]};
}
export async function removeOperations(input, project) {
  const old = await read(input.id);
  if (!old.present) throw fault("not_found", "Task is unavailable");
  owned(old.value, project);
  if (old.revision !== revision(input.revision)) throw fault("revision_conflict", "Task changed");
  return {item:old.value,operations:[{kind:"remove",data:{key:key(input.id),expected_revision:old.revision}},...await removal(input.id)]};
}
export function transactionResult(output, id, removed) {
  const entry = output?.kind === "plugin_transaction" && Array.isArray(output.data)
    ? output.data.find(item => item.kind === "plugin_value" && item.data?.key === key(id))?.data : null;
  if (!entry || entry.present !== !removed) throw fault("outcome_unknown", "Task outcome is unavailable");
  revision(entry.revision);
  if (!removed && entry.value?.id !== id) throw fault("outcome_unknown", "Task outcome is unavailable");
  return removed ? {id,revision:entry.revision,removed:true} : {...draft(entry.value),revision:entry.revision};
}
