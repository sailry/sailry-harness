import { context, prepareRequest, completeRequest, forgetRequest } from "sailry/sdk";
import { fault } from "./records.js";

async function query(kind) {
  const id = prepareRequest({kind:"dispatch", data:{package:context().package, action:{kind}}});
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
  const schedules = await query("list_schedules");
  const handlers = await query("list_handlers");
  const operations = [];
  let schedule = schedules.find(schedule => schedule.id === item.id);
  const handler = handlers.find(handler => handler.name === item.id);
  const enabled = item.due_ms !== null && !item.completed && item.notified_ms === null;
  if (enabled && schedule && schedule.next_ms === null) {
    operations.push(dispatch("remove_schedule", {id:item.id, expected_revision:exact(schedule.revision)}));
    schedule = null;
  }
  operations.push(dispatch("save_schedule", {
    id:item.id,revision:schedule ? exact(schedule.revision) : "0",enabled,topic:item.id,
    payload:{id:item.id, revision:nextRevision},timing:{kind:"once",data:{at_ms:item.due_ms ?? 0}},next_ms:null,
  }));
  operations.push(dispatch("save_handler", {
    name:item.id,revision:handler ? exact(handler.revision) : "0",enabled:true,
    source:{package:context().package.name,topic:item.id},queue:"reminders",
    callback:{scope:{worktree:null,session:null},completion:"command",
      command:{kind:"call_plugin",data:{handler:"notify",input:null}},bindings:{"/data/input":"/payload"}},
  }));
  return operations;
}
export async function removal(id) {
  const schedules = await query("list_schedules");
  const handlers = await query("list_handlers");
  const schedule = schedules.find(schedule => schedule.id === id);
  const handler = handlers.find(handler => handler.name === id);
  return [
    ...(schedule ? [dispatch("remove_schedule", {id,expected_revision:exact(schedule.revision)})] : []),
    ...(handler ? [dispatch("remove_handler", {name:id,expected_revision:exact(handler.revision)})] : []),
  ];
}
