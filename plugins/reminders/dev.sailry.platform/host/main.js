import { key, read, revision, commit, page, result } from "./records.js";
import {saveOperations, removeOperations} from "./mutations.js";
export {initialize, reminders} from "./tools.js";

export async function list(input) { return result(() => page(input ?? {})); }
export async function save(input) {
  return result(async () => {
    const {item, operations} = await saveOperations(input);
    const outputs = await commit(operations);
    return {...item,revision:outputs[0].data.revision};
  });
}
export async function remove(input) {
  return result(async () => {
    await commit((await removeOperations(input)).operations);
    return {removed:true};
  });
}
export async function notify(input) {
  const old = await read(input.id);
  const expected = revision(input.revision);
  const now = Date.now();
  if (!old.present || old.revision !== expected || old.value.completed
      || old.value.notified_ms !== null || old.value.due_ms === null || old.value.due_ms > now) {
    return {skipped:true};
  }
  const item = {...old.value,notified_ms:now};
  try {
    await commit([
      {kind:"write",data:{key:key(input.id),value:item,expected_revision:expected}},
      {kind:"notify",data:{title:item.title,message:item.message,kind:"info",session:null}},
    ]);
  } catch (error) {
    if (error.code === "revision_conflict") return {skipped:true};
    throw error;
  }
  return {delivered:true};
}
