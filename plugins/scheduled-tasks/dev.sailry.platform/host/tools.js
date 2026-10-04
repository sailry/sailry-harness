import {context, callId, readTurnState, stageTurnState} from "sailry/sdk";
import {read, fault} from "./records.js";
import {listing, saveOperations, removeOperations, transactionResult} from "./mutations.js";

export function initialize({project, tools, config}) {
  if (!config || config.assistant || config.resource) return {instruction:"",tools:[]};
  stageTurnState({project,worktree:project === null ? null : context().worktree,config});
  const now = Date.now();
  return {instruction:`Scheduled tasks use UTC timestamps in milliseconds. Current UTC: ${new Date(now).toISOString()} (${now} ms). Ask for the schedule and timezone if missing. Repeating intervals are fixed durations, not local calendar rules.`,tools};
}
function argumentsFor(value) {
  const fields = {list:["after"],create:["name","prompt","timing","enabled"],
    update:["id","revision","name","prompt","timing","enabled"],delete:["id","revision"]};
  if (!value || typeof value !== "object" || Array.isArray(value) || !Object.hasOwn(fields,value.action)
      || Object.keys(value).some(key => key !== "action" && !fields[value.action].includes(key))) {
    throw fault("invalid_request", "Invalid scheduled task action or arguments");
  }
  if (value.action === "create" && ["name","prompt","timing"].some(key => !Object.hasOwn(value,key))) {
    throw fault("invalid_request", "Task name, prompt and timing are required");
  }
  if (["update","delete"].includes(value.action) && (typeof value.id !== "string" || typeof value.revision !== "string")) {
    throw fault("invalid_request", "Task ID and revision are required");
  }
  if (value.action === "update" && !["name","prompt","timing","enabled"].some(key => Object.hasOwn(value,key))) {
    throw fault("invalid_request", "Task update requires a change");
  }
  if (Object.hasOwn(value,"timing")) {
    const timing=value.timing, once=timing?.kind === "once", every=timing?.kind === "every";
    const fields=once ? ["at_ms"] : ["anchor_ms","interval_ms"];
    if (!timing || typeof timing !== "object" || Array.isArray(timing) || (!once && !every)
        || Object.keys(timing).some(key => !["kind","data"].includes(key))
        || !timing.data || typeof timing.data !== "object" || Array.isArray(timing.data)
        || Object.keys(timing.data).some(key => !fields.includes(key))) {
      throw fault("invalid_request", "Invalid task timing");
    }
    const first=once ? timing.data.at_ms : timing.data.anchor_ms;
    if (!Number.isSafeInteger(first) || first <= Date.now()) throw fault("invalid_request", "Task start time must be in the future");
  }
  return value;
}
export async function scheduled_tasks({arguments:input, step, state, outcome}) {
  try {
    if (step === 1) {
      if (outcome?.isError) return {result:outcome};
      return {result:transactionResult(outcome,state.id,state.removed)};
    }
    if (step !== 0) throw fault("invalid_request", "Invalid scheduled task continuation");
    const args=argumentsFor(input), scope=readTurnState();
    if (!scope?.config || scope.project === undefined) throw fault("not_configured", "Task session configuration is unavailable");
    if (args.action === "list") return {result:await listing(args,scope.project)};
    let pending;
    if (args.action === "create") {
      pending=await saveOperations({id:callId(),revision:"0",name:args.name,prompt:args.prompt,timing:args.timing,
        enabled:args.enabled === undefined ? true : args.enabled,queue:"default",...scope},scope.project);
    } else if (args.action === "update") {
      const old=await read(args.id);
      if (!old.present) throw fault("not_found", "Task is unavailable");
      if (old.value.project !== scope.project) throw fault("permission_denied", "Task belongs to another project");
      const changes=Object.fromEntries(["name","prompt","timing","enabled"].filter(key => Object.hasOwn(args,key)).map(key => [key,args[key]]));
      pending=await saveOperations({...old.value,...changes,revision:args.revision},scope.project);
    } else pending=await removeOperations(args,scope.project);
    return {call:{operation:"storage.transaction",arguments:{operations:pending.operations}},
      state:{id:pending.item.id,removed:args.action === "delete"}};
  } catch (error) { return {result:{isError:true,error:{code:error.code ?? "invalid_request",message:error.message}}}; }
}
