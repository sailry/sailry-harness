import {context, listTeams, prepareRequest, completeRequest, forgetRequest} from "sailry/sdk";
import {execution, fault, requireConfig, node} from "./records.js";

export async function resolve(model, effort, config) {
  const id = prepareRequest({kind:"resolve_plugin_model",data:{model,effort,config}});
  const result = await completeRequest(id);
  forgetRequest(id);
  if (result.Err) throw fault(result.Err.code, result.Err.message);
  return result.Ok.data;
}
export async function executor(item) {
  if (item.execution_node != null) return node(item.execution_node);
  const [schedules, handlers] = await Promise.all([query("list_schedules"), query("list_handlers")]);
  if (!schedules.some(schedule => schedule.id === item.id) && !handlers.some(handler => handler.name === item.id)) {
    throw fault("unavailable", "Task execution Node is unknown");
  }
  return node(context().execution_node);
}
export async function configuration(config, team = null) {
  requireConfig(config);
  const selected = team ? (await listTeams()).find(item => item.id === team.id) : null;
  if (team && !selected) throw fault("not_found", "Task team is unavailable");
  const fixed = selected?.model;
  if (!fixed) return resolve(`${config.provider}/${config.model}`,config.effort,config);
  const model = `${fixed.provider}/${fixed.model}`;
  // Resolve the model's default at its owner, even when the base uses that model.
  const effort = fixed.effort ?? (await resolve(model,null,null)).effort;
  return resolve(model,effort,config);
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
export function registration(task, revision) {
  const source = context();
  return operation("save_handler",{name:task.id,revision,enabled:true,
    source:{package:source.package.name,topic:task.id},queue:task.queue,
    callback:{scope:{worktree:source.worktree ?? null,session:source.session ?? null},completion:"turn",bindings:{},
      command:{kind:"start_session",data:{project:task.project,worktree:task.worktree,config:task.config,
        ...execution(task),association:{namespace:context().package.name,kind:"task",id:task.id},
        title:task.name,message:{text:task.prompt,attachments:[],references:[]}}}}});
}
export async function changes(task) {
  requireConfig(task.config);
  const [schedules,handlers] = await Promise.all([query("list_schedules"),query("list_handlers")]);
  const schedule = schedules.find(item => item.id === task.id);
  const handler = handlers.find(item => item.name === task.id);
  return [
    registration(task,handler ? exact(handler.revision) : "0"),
    operation("save_schedule",{id:task.id,revision:schedule ? exact(schedule.revision) : "0",
      enabled:task.enabled,topic:task.id,payload:{name:task.name},timing:task.timing,next_ms:null}),
  ];
}
export async function removal(id) {
  const [schedules,handlers] = await Promise.all([query("list_schedules"),query("list_handlers")]);
  const schedule = schedules.find(item => item.id === id);
  const handler = handlers.find(item => item.name === id);
  return [
    ...(schedule ? [operation("remove_schedule",{id,expected_revision:exact(schedule.revision)})] : []),
    ...(handler ? [operation("remove_handler",{name:id,expected_revision:exact(handler.revision),cancel_pending:true})] : []),
  ];
}
