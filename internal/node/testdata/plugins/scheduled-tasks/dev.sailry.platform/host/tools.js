import {context, callId, readTurnState, stageTurnState} from "sailry/sdk";
import {read, fault, timing, execution, sameNode, hostCall, snapshot} from "./records.js";
import {listing, saveOperations, removeOperations, transactionResult} from "./mutations.js";
import {executor} from "./dispatch.js";

export function initialize({project, tools, config, execution:mode, team, models}) {
  if (!config || config.assistant || config.resource) return {instruction:"",tools:[]};
  stageTurnState({project,worktree:project === null ? null : context().worktree,config,
    ...execution({execution:mode,team,models})});
  return {instruction:"Use the turn's UTC start time for relative times when available. Otherwise establish the current time before resolving a relative schedule. Ask for the schedule and timezone if missing. Once/every use UTC milliseconds; every is a fixed elapsed interval. Weekly uses an explicit IANA timezone, ISO weekdays (Monday=1 through Sunday=7), hour and minute. It runs once at the earlier instant of a repeated local time and skips a missing local time.",tools};
}
function argumentsFor(value) {
  const fields = {list:["after"],create:["name","prompt","timing","enabled","execution","team"],
    update:["id","revision","name","prompt","timing","enabled","execution","team"],delete:["id","revision"]};
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
  if (value.action === "update" && !["name","prompt","timing","enabled","execution","team"].some(key => Object.hasOwn(value,key))) {
    throw fault("invalid_request", "Task update requires a change");
  }
  if (Object.hasOwn(value,"timing")) {
    const schedule = timing(value.timing, true);
    if (schedule.kind !== "weekly") {
      const first = schedule.kind === "once" ? schedule.data.at_ms : schedule.data.anchor_ms;
      if (first <= Date.now()) throw fault("invalid_request", "Task start time must be in the future");
    }
    return {...value,timing:schedule};
  }
  return value;
}
function select(input, previous) {
  if (!Object.hasOwn(input,"execution") && !Object.hasOwn(input,"team")) return previous;
  const mode = Object.hasOwn(input,"execution") ? input.execution : Object.hasOwn(input,"team") ? "team" : previous.execution;
  const team = Object.hasOwn(input,"team") ? input.team : mode === "model" ? null : previous.team;
  const next = execution({execution:mode,team}, true);
  const unchanged = previous.execution === next.execution && previous.team?.id === next.team?.id;
  return {...previous,...next,models:unchanged ? previous.models : {}};
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
        enabled:args.enabled === undefined ? true : args.enabled,queue:"default",...select(args,scope)},scope.project);
    } else if (args.action === "update") {
      const old=await read(args.id);
      if (!old.present) throw fault("not_found", "Task is unavailable");
      if (old.value.project !== scope.project) throw fault("permission_denied", "Task belongs to another project");
      const changes=Object.fromEntries(["name","prompt","timing","enabled"].filter(key => Object.hasOwn(args,key)).map(key => [key,args[key]]));
      const target = await executor(old.value);
      if (!sameNode(target, context().execution_node)) {
        const current = await hostCall(target, "record", {id:args.id}, true);
        if (!current) throw fault("not_found", "Task is unavailable");
        if (current.project !== scope.project) throw fault("permission_denied", "Task belongs to another project");
        if (current.revision !== args.revision) throw fault("revision_conflict", "Task changed");
        return {result:await hostCall(target,"save",{...select(args,current),...changes,execution_node:target,revision:current.revision,
          expected_value:snapshot(current)})};
      }
      pending=await saveOperations({...select(args,old.value),...changes,revision:args.revision},scope.project);
    } else {
      const old=await read(args.id);
      if (!old.present) throw fault("not_found", "Task is unavailable");
      if (old.value.project !== scope.project) throw fault("permission_denied", "Task belongs to another project");
      const target = await executor(old.value);
      if (!sameNode(target, context().execution_node)) {
        const current = await hostCall(target, "record", {id:args.id}, true);
        if (!current) throw fault("not_found", "Task is unavailable");
        if (current.project !== scope.project) throw fault("permission_denied", "Task belongs to another project");
        if (current.revision !== args.revision) throw fault("revision_conflict", "Task changed");
        return {result:await hostCall(target, "remove", {...args, revision:current.revision, expected_value:snapshot(current)})};
      }
      pending=await removeOperations(args,scope.project);
    }
    return {call:{operation:"storage.transaction",arguments:{operations:pending.operations}},
      state:{id:pending.item.id,removed:args.action === "delete"}};
  } catch (error) { return {result:{isError:true,error:{code:error.code ?? "invalid_request",message:error.message}}}; }
}
