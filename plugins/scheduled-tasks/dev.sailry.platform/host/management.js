import {context, getValue, readProjectCatalog, prepareRequest, completeRequest, forgetRequest} from "sailry/sdk";
import {key, commit, result, fault} from "./records.js";
import {query, configuration} from "./dispatch.js";
import {listing, saveOperations, removeOperations} from "./mutations.js";

export async function projects() { return result(() => readProjectCatalog()); }
export async function list(input) {
  return result(() => listing(input));
}
// Read native job state independently of the Records tab's visible page count.
export async function states() {
  const states = new Map();
  let before = null;
  do {
    const page = await query("list_jobs",{before,limit:100});
    for (const job of page.jobs) {
      if (!states.has(job.handler) || job.status === "running"
          || (job.status === "queued" && states.get(job.handler) !== "running")) {
        states.set(job.handler,job.status);
      }
    }
    before = page.next_before;
  } while (before !== null);
  return Object.fromEntries(states);
}
export async function save(input) {
  return result(async () => {
    const pending = await saveOperations(input);
    const outputs = await commit(pending.operations);
    return {...pending.item,revision:outputs[0].data.revision};
  });
}
export async function remove(input) {
  return result(async () => {
    const pending = await removeOperations(input);
    await commit(pending.operations);
    return {removed:true};
  });
}
export async function run(input) {
  return result(async () => {
    const old = await getValue(key(input.id));
    if (!old.present) throw fault("not_found", "Task is unavailable");
    await configuration(old.value.config);
    return query("enqueue",{handler:input.id,payload:{name:old.value.name}});
  });
}
export async function cancel(input) { return result(() => query("cancel_job",{id:input.id})); }
export async function stop(input) {
  return result(async () => {
    const id = prepareRequest({kind:"stop_dispatch_turn",data:{package:context().package,job:input.id}});
    const receipt = await completeRequest(id);
    forgetRequest(id);
    if (receipt.Err) throw fault(receipt.Err.code, receipt.Err.message);
    return receipt.Ok.data;
  });
}
export async function history(input = {}) {
  return result(async () => {
    const page = await query("list_jobs",{before:input.before ?? null,limit:input.limit ?? 32});
    const items = [];
    for (const job of page.jobs) {
      const receipt = await query("read_result",{id:job.id});
      const output = receipt.kind === "completed" ? receipt.data.Ok : null;
      const turn = output?.kind === "queued_turn" ? output.data : null;
      items.push({job,name:job.event.payload?.name ?? job.handler,session:turn?.session ?? null,turn:turn?.id ?? null});
    }
    return {items,next_before:page.next_before};
  });
}
