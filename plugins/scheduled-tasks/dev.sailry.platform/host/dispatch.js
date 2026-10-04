import {context, prepareRequest, completeRequest, forgetRequest} from "sailry/sdk";
import {fault, requireConfig} from "./records.js";

export async function configuration(config) {
  requireConfig(config);
  const id = prepareRequest({kind:"resolve_plugin_model",data:{model:`${config.provider}/${config.model}`,
    effort:config.effort,config}});
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data;
}

export async function query(kind, data) {
  const id = prepareRequest({kind:"dispatch",data:{package:context().package,action:{kind,data}}});
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data.data;
}
export function operation(kind, data) { return {kind:"dispatch",data:{kind,data}}; }
export function exact(value) {
  if (!Number.isSafeInteger(value) || value < 0) throw fault("invalid_request", "Invalid dispatch revision");
  return String(value);
}
export async function changes(task) {
  requireConfig(task.config);
  const schedules = await query("list_schedules");
  const handlers = await query("list_handlers");
  const schedule = schedules.find(item => item.id === task.id);
  const handler = handlers.find(item => item.name === task.id);
  return [
    operation("save_schedule",{id:task.id,revision:schedule ? exact(schedule.revision) : "0",
      enabled:task.enabled,topic:task.id,payload:{name:task.name},timing:task.timing,next_ms:null}),
    operation("save_handler",{name:task.id,revision:handler ? exact(handler.revision) : "0",enabled:true,
      source:{package:context().package.name,topic:task.id},queue:task.queue,
      callback:{scope:{worktree:null,session:null},completion:"turn",bindings:{},
        command:{kind:"start_session",data:{project:task.project,worktree:task.worktree,config:task.config,
          title:task.name,message:{text:task.prompt,attachments:[],references:[]}}}}}),
  ];
}
export async function removal(id) {
  const schedules = await query("list_schedules");
  const handlers = await query("list_handlers");
  const schedule = schedules.find(item => item.id === id);
  const handler = handlers.find(item => item.name === id);
  return [
    ...(schedule ? [operation("remove_schedule",{id,expected_revision:exact(schedule.revision)})] : []),
    ...(handler ? [operation("remove_handler",{name:id,expected_revision:exact(handler.revision),cancel_pending:true})] : []),
  ];
}
