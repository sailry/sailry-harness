import { context, prepareRequest, completeRequest, forgetRequest } from "sailry/sdk";
import { fault, key, result, read, node, sameNode, hostCall, snapshot } from "./records.js";

async function query(kind, data) {
  const action = data === undefined ? {kind} : {kind, data};
  const id = prepareRequest({kind:"dispatch", data:{package:context().package, action}});
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data.data;
}
function dispatch(kind, data) { return {kind:"dispatch", data:{kind, data}}; }
function exact(value) {
  if (!Number.isSafeInteger(value) || value < 0) throw fault("invalid_request", "Invalid schedule revision");
  return String(value);
}
export async function changes(item, nextRevision) {
  const source = context();
  const [schedules,handlers] = await Promise.all([query("list_schedules"),query("list_handlers")]);
  const operations = [];
  let schedule = schedules.find(schedule => schedule.id === item.id);
  const handler = handlers.find(handler => handler.name === item.id);
  const enabled = item.due_ms !== null && !item.completed && item.notified_ms === null;
  if (enabled && schedule && schedule.next_ms === null) {
    operations.push(dispatch("remove_schedule", {id:item.id, expected_revision:exact(schedule.revision)}));
    schedule = null;
  }
  operations.push(dispatch("save_handler", {
    name:item.id,revision:handler ? exact(handler.revision) : "0",enabled:true,
    source:{package:context().package.name,topic:item.id},queue:"reminders",
    callback:{scope:{worktree:source.worktree ?? null,session:source.session ?? null},completion:"command",
      command:{kind:"call_plugin",data:{handler:"notify",input:null}},bindings:{"/data/input":"/payload"}},
  }));
  operations.push(dispatch("save_schedule", {
    id:item.id,revision:schedule ? exact(schedule.revision) : "0",enabled,topic:item.id,
    payload:{id:item.id, revision:nextRevision, expected_value:snapshot(item)},
    timing:{kind:"once",data:{at_ms:item.due_ms ?? 0}},next_ms:null,
  }));
  return operations;
}
export async function removal(id) {
  const [schedules,handlers] = await Promise.all([query("list_schedules"),query("list_handlers")]);
  const schedule = schedules.find(schedule => schedule.id === id);
  const handler = handlers.find(handler => handler.name === id);
  return [
    ...(schedule ? [dispatch("remove_schedule", {id,expected_revision:exact(schedule.revision)})] : []),
    ...(handler ? [dispatch("remove_handler", {name:id,expected_revision:exact(handler.revision)})] : []),
  ];
}
// Legacy records have no owner field. Only dispatch records on this Node prove
// ownership; seeing a personal KV copy never authorizes a replacement schedule.
export async function execution(item) {
  if (item.execution_node != null) return node(item.execution_node);
  const [schedules, handlers] = await Promise.all([query("list_schedules"), query("list_handlers")]);
  if (!schedules.some(schedule => schedule.id === item.id) && !handlers.some(handler => handler.name === item.id)) {
    throw fault("unavailable", "Reminder execution Node is unknown");
  }
  return node(context().execution_node);
}
// Native notification jobs for one reminder, newest first. Callbacks that skipped a
// stale schedule delivered nothing and are omitted.
export async function history(input = {}) {
  return result(async () => {
    const handler = input.handler;
    key(handler);
    const record = await read(handler);
    if (record.present) {
      const target = await execution(record.value);
      if (!sameNode(target, context().execution_node)) return hostCall(target, "history", {handler}, true);
    }
    const items = [];
    let before = null;
    do {
      const page = await query("list_jobs", {before, limit:100});
      const jobs=page.jobs.filter(job=>job.handler===handler);
      const visible=await Promise.all(jobs.map(async job=>{
        if(job.status!=="completed")return job;
        const receipt=await query("read_result",{id:job.id});
        return receipt?.kind==="completed"&&receipt.data?.Ok?.data?.skipped===true?null:job;
      }));
      items.push(...visible.filter(job=>job!==null));
      before = page.next_before;
    } while (before !== null);
    return {items};
  });
}
