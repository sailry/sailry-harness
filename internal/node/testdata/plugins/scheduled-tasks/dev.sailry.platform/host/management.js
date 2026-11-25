import {context, readProjectCatalog, listTeams, prepareRequest, completeRequest, forgetRequest} from "sailry/sdk";
import {key, read, commit, result, fault, node, sameNode, hostCall, snapshot, checkBase} from "./records.js";
import {query, configuration, registration, operation, exact, executor, resolve} from "./dispatch.js";
import {listing, saveOperations, removeOperations} from "./mutations.js";

export async function projects() { return result(() => readProjectCatalog()); }
export async function options() {
  return result(async () => {
    const modelsRequest = prepareRequest({kind:"list_plugin_models"});
    const [outcome, teams] = await Promise.all([completeRequest(modelsRequest), listTeams()]);
    forgetRequest(modelsRequest);
    if (outcome.Err) throw fault(outcome.Err.code,outcome.Err.message);
    const models = outcome.Ok.data;
    return {models:models.models.filter(model => model.kind === "provider"), teams};
  });
}
export async function model(input) { return result(() => resolve(input.model, input.effort, input.config)); }
export async function runtime() {
  return result(async () => {
    const [status, schedules] = await Promise.all([states(), query("list_schedules")]);
    return {status, schedules};
  });
}
export async function list(input) {
  return result(() => listing(input));
}
export async function record(input) {
  return result(async () => {
    const entry = await read(input.id);
    if (!entry.present) return null;
    const target = await executor(entry.value);
    if (!sameNode(target, context().execution_node)) throw fault("conflict", "Task belongs to another execution Node");
    return {...entry.value, execution_node:target, revision:entry.revision};
  });
}
// Read native job state independently of the visible record pages.
export async function states() {
  const states = new Map();
  let before = null;
  do {
    const page = await query("list_jobs",{before,limit:100});
    for (const job of page.jobs) {
      const known = states.get(job.handler)?.status;
      if (!known || job.status === "running" || (job.status === "queued" && known !== "running")) {
        states.set(job.handler,{status:job.status,started_ms:job.started_ms ?? null});
      }
    }
    before = page.next_before;
  } while (before !== null);
  return Object.fromEntries(states);
}
export async function save(input) {
  return result(async () => {
    let old;
    if (input.id != null) {
      old = await read(input.id);
      if (old.present) {
        const target = await executor(old.value);
        if (input.execution_node != null && !sameNode(input.execution_node, target)) {
          throw fault("conflict", "Task remains on its original execution Node");
        }
        input = {...input,execution_node:target};
      }
    }
    const target = node(input.execution_node ?? context().execution_node);
    if (!sameNode(target, context().execution_node)) {
      if (old?.present && !Object.hasOwn(input, "expected_value")) input = {...input, expected_value:snapshot(old.value)};
      return hostCall(target, "save", input);
    }
    const pending = await saveOperations(input);
    const outputs = await commit(pending.operations);
    return {...pending.item,revision:outputs[0].data.revision};
  });
}
export async function remove(input) {
  return result(async () => {
    const old = await read(input.id);
    if (old.present) {
      const target = await executor(old.value);
      if (!sameNode(target, context().execution_node)) {
        if (!Object.hasOwn(input, "expected_value")) input = {...input, expected_value:snapshot(old.value)};
        return hostCall(target, "remove", input);
      }
    }
    const pending = await removeOperations(input);
    await commit(pending.operations);
    return {removed:true};
  });
}
export async function run(input) {
  return result(async () => {
    key(input.id);
    const record = await read(input.id);
    if (!record.present) throw fault("not_found", "Task is unavailable");
    const target = await executor(record.value);
    if (!sameNode(target, context().execution_node)) {
      const forwarded = {...input, revision:input.revision ?? record.revision};
      if (!Object.hasOwn(forwarded, "expected_value")) forwarded.expected_value = snapshot(record.value);
      return hostCall(target, "run", forwarded);
    }
    if (input.revision != null || Object.hasOwn(input, "expected_value")) checkBase(input, record);
    // The KV fence also catches source snapshots that change the local view
    // without editing this Node's handler.
    const handler = (await query("list_handlers")).find(handler => handler.name === input.id);
    const old = await read(input.id);
    if (!old.present) throw fault("not_found", "Task is unavailable");
    if (input.revision != null || Object.hasOwn(input, "expected_value")) checkBase(input, old);
    const current = {...old.value, config:await configuration(old.value.config,old.value.team)};
    if (!handler) throw fault("not_found", "Task handler is unavailable");
    const outputs = await commit([
      {kind:"write", data:{key:key(input.id), value:old.value, expected_revision:old.revision}},
      registration(current,exact(handler.revision)),
      operation("enqueue",{handler:input.id,payload:{name:old.value.name}}),
    ]);
    const output = outputs[2];
    if (output?.kind !== "dispatch" || output.data?.kind !== "job") {
      throw fault("outcome_unknown", "Task run outcome is unavailable");
    }
    return output.data.data;
  });
}
export async function cancel(input) {
  return result(async () => {
    if (input.execution_node != null && !sameNode(input.execution_node, context().execution_node)) {
      return hostCall(node(input.execution_node), "cancel", input);
    }
    return query("cancel_job",{id:input.id});
  });
}
export async function stop(input) {
  return result(async () => {
    if (input.execution_node != null && !sameNode(input.execution_node, context().execution_node)) {
      return hostCall(node(input.execution_node), "stop", input);
    }
    const id = prepareRequest({kind:"stop_dispatch_turn",data:{package:context().package,job:input.id}});
    const receipt = await completeRequest(id);
    forgetRequest(id);
    if (receipt.Err) throw fault(receipt.Err.code, receipt.Err.message);
    return receipt.Ok.data;
  });
}
// A handler filter scans whole native pages until it has a page of that task's runs.
export async function history(input = {}) {
  return result(async () => {
    const limit = input.limit ?? 32, handler = input.handler ?? null;
    if (input.execution_node != null && !sameNode(input.execution_node, context().execution_node)) {
      return hostCall(node(input.execution_node), "history", input, true);
    }
    if (handler !== null) {
      const old = await read(handler);
      if (old.present) {
        const target = await executor(old.value);
        if (!sameNode(target, context().execution_node)) return hostCall(target, "history", {...input,execution_node:target}, true);
      }
    }
    const items = [];
    let before = input.before ?? null;
    do {
      const page = await query("list_jobs",{before,limit:handler === null ? limit : 100});
      const entries=await Promise.all(page.jobs.filter(job=>handler===null||job.handler===handler).map(async job=>{
        const receipt = await query("read_result",{id:job.id});
        const output = receipt.kind === "completed" ? receipt.data.Ok : null;
        const turn = output?.kind === "queued_turn" ? output.data : null;
        return {job,execution_node:node(context().execution_node),name:job.event.payload?.name ?? job.handler,session:turn?.session ?? null,turn:turn?.id ?? null};
      }));
      items.push(...entries);
      before = page.next_before;
    } while (handler !== null && items.length < limit && before !== null);
    return {items,next_before:before};
  });
}
