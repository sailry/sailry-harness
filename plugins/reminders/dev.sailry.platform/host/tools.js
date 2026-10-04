import {callId, readTurnState, stageTurnState} from "sailry/sdk";
import {project, page, read, fault} from "./records.js";
import {saveOperations, removeOperations, transactionResult} from "./mutations.js";

export function initialize({project:scope, tools}) {
  project(scope);
  stageTurnState({project:scope});
  return {instruction:"", tools:scope === null ? [] : tools};
}
function argumentsFor(value) {
  const fields = {list:["after"], create:["title","message","completed","due_ms"],
    update:["id","revision","title","message","completed","due_ms"], delete:["id","revision"]};
  if (!value || typeof value !== "object" || Array.isArray(value) || !Object.hasOwn(fields, value.action)
      || Object.keys(value).some(key => key !== "action" && !fields[value.action].includes(key))) {
    throw fault("invalid_request", "Invalid reminder action or arguments");
  }
  if (value.action === "create" && typeof value.title !== "string") throw fault("invalid_request", "Reminder title is required");
  if (["update","delete"].includes(value.action) && (typeof value.id !== "string" || typeof value.revision !== "string")) {
    throw fault("invalid_request", "Reminder ID and revision are required");
  }
  if (value.action === "update" && !["title","message","completed","due_ms"].some(key => Object.hasOwn(value,key))) {
    throw fault("invalid_request", "Reminder update requires a change");
  }
  return value;
}
export async function reminders({arguments:input, step, state, outcome}) {
  try {
    if (step === 1) {
      if (outcome?.isError) return {result:outcome};
      return {result:transactionResult(outcome, state.id, state.removed)};
    }
    if (step !== 0) throw fault("invalid_request", "Invalid reminder continuation");
    const args = argumentsFor(input), scope = project(readTurnState()?.project);
    if (scope === null) throw fault("permission_denied", "Reminders require a current project");
    if (args.action === "list") return {result:await page(args, scope)};
    let pending;
    if (args.action === "create") {
      pending = await saveOperations({id:callId(),revision:"0",project:scope,title:args.title,
        message:args.message === undefined ? "" : args.message,
        completed:args.completed === undefined ? false : args.completed,
        due_ms:args.due_ms === undefined ? null : args.due_ms}, scope);
    } else if (args.action === "update") {
      const old = await read(args.id);
      if (!old.present) throw fault("not_found", "Reminder is unavailable");
      if (old.value.project !== scope) throw fault("permission_denied", "Reminder belongs to another project");
      const changes = Object.fromEntries(["title","message","completed","due_ms"]
        .filter(key => Object.hasOwn(args,key)).map(key => [key,args[key]]));
      pending = await saveOperations({...old.value,...changes,revision:args.revision}, scope);
    } else pending = await removeOperations(args, scope);
    return {call:{operation:"storage.transaction",arguments:{operations:pending.operations}},
      state:{id:pending.item.id,removed:args.action === "delete"}};
  } catch (error) { return {result:{isError:true,error:{code:error.code ?? "invalid_request",message:error.message}}}; }
}
