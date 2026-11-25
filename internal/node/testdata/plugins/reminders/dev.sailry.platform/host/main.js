import {context} from "sailry/sdk";
import { key, read, commit, page, result, node, sameNode, hostCall, stored, snapshot, checkBase } from "./records.js";
import {saveOperations, removeOperations} from "./mutations.js";
import {presentation} from "./locales.js";
import {execution} from "./schedules.js";
export {initialize, reminders} from "./tools.js";
export {history} from "./schedules.js";

export async function list(input) { return result(() => page(input ?? {})); }
export async function record(input) {
  return result(async () => {
    const entry = await read(input.id);
    if (!entry.present) return null;
    const target = await execution(entry.value);
    if (!sameNode(target, context().execution_node)) throw Object.assign(new Error("Reminder belongs to another execution Node"), {code:"conflict"});
    return {...stored(entry.value, input.id), execution_node:target, revision:entry.revision};
  });
}
export async function save(input) {
  return result(async () => {
    let old;
    if (input.id != null) {
      old = await read(input.id);
      if (old.present) {
        const target = await execution(old.value);
        if (input.execution_node != null && !sameNode(target, input.execution_node)) {
          throw Object.assign(new Error("Reminder remains on its original execution Node"), {code:"conflict"});
        }
        input = {...input, execution_node:target};
      }
    }
    const target = node(input.execution_node ?? context().execution_node);
    if (!sameNode(target, context().execution_node)) {
      if (old?.present && !Object.hasOwn(input, "expected_value")) input = {...input, expected_value:snapshot(old.value)};
      return hostCall(target, "save", input);
    }
    const {item, operations} = await saveOperations(input);
    const outputs = await commit(operations);
    return {...item,revision:outputs[0].data.revision};
  });
}
export async function remove(input) {
  return result(async () => {
    const old = await read(input.id);
    if (old.present) {
      const target = await execution(old.value);
      if (!sameNode(target, context().execution_node)) {
        if (!Object.hasOwn(input, "expected_value")) input = {...input, expected_value:snapshot(old.value)};
        return hostCall(target, "remove", input);
      }
    }
    await commit((await removeOperations(input)).operations);
    return {removed:true};
  });
}
export async function notify(input) {
  const old = await read(input.id);
  const now = Date.now();
  if (!old.present || old.value.completed
      || old.value.notified_ms !== null || old.value.due_ms === null || old.value.due_ms > now) {
    return {skipped:true};
  }
  let expected;
  try { expected = checkBase(input, old); }
  catch (error) {
    if (error.code === "revision_conflict") return {skipped:true};
    throw error;
  }
  const target = await execution(old.value);
  if (!sameNode(target, context().execution_node)) return {skipped:true};
  const item = {...old.value,execution_node:target,notified_ms:now};
  try {
    await commit([
      {kind:"write",data:{key:key(input.id),value:item,expected_revision:expected}},
      {kind:"notify",data:{title:item.title,message:item.message,kind:"info",session:null,presentation}},
    ]);
  } catch (error) {
    if (error.code === "revision_conflict") return {skipped:true};
    throw error;
  }
  return {delivered:true};
}
