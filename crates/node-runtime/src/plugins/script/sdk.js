// The Node owns admission, scope checks and persistence; this module only adapts values.
const bridge = globalThis.__sailry;
delete globalThis.__sailry;
function call(operation, value) {
  const result = JSON.parse(bridge(operation, JSON.stringify(value)));
  if (result.Err) {
    const error = new Error(result.Err.message);
    error.code = result.Err.code;
    throw error;
  }
  return result.Ok;
}
const scope = call("context", null);
export function context() { return JSON.parse(JSON.stringify(scope)); }
export async function readProjectCatalog() { return read("read_project_catalog"); }
export async function readSession() { return read("read_session", {session:scope.session}); }
export async function readConversation(before=null, limit=20) { return read("read_conversation", {session:scope.session,before,limit}); }
export async function readTurn(turn,before=null,limit=100) {
  const history=await readConversation(null,1);
  return read("read_turn", {session:scope.session,turn,expected_revision:history.page.revision,before,limit});
}
export function startSession(draft) { return prepareRequest({kind:"start_session", data:draft}); }
export function prepareRequest(command) { return call("prepare", command); }
export function prepareTransaction(operations) {
  return prepareRequest({kind: "plugin_transaction", data: {operations}});
}
export async function completeRequest(id) { return call("complete", id); }
export function forgetRequest(id) { call("forget", id); }
async function read(kind, data) { return call("read", {kind, data}).data; }
export async function readSettings() { return read("read_plugin_settings", {package: scope.package}); }
export async function getValue(key) { return read("read_plugin_value", {key}); }
export async function getConversationValue(key) { return read("read_plugin_conversation_value", {key}); }
export async function listKeys(prefix = "", after = null, limit = 64) {
  return read("list_plugin_keys", {prefix, after, limit});
}
export function setValue(key, value, expectedRevision) {
  return prepareRequest({kind: "write_plugin_value", data: {key, value, expected_revision: expectedRevision}});
}
export function setConversationValue(key, value, expectedRevision) {
  return prepareRequest({kind: "write_plugin_conversation_value", data: {key, value, expected_revision: expectedRevision}});
}
export function indexedValue(key, value, index, expectedRevision) {
  return prepareRequest({kind: "write_indexed_plugin_value", data: {key, value, index, expected_revision: expectedRevision}});
}
export async function searchValues(query) { return read("search_plugin_values", query); }
export function deleteValue(key, expectedRevision) {
  return prepareRequest({kind: "remove_plugin_value", data: {key, expected_revision: expectedRevision}});
}
export function deleteConversationValue(key, expectedRevision) {
  return prepareRequest({kind: "remove_plugin_conversation_value", data: {key, expected_revision: expectedRevision}});
}
export function prepareNotification(content) {
  return prepareRequest({kind: "publish_notification", data: {package: scope.package, content}});
}
export async function inspectGit() { return read("inspect_git", {worktree: scope.worktree}); }
export async function readFile(path) { return read("read_file", {worktree: scope.worktree, path}); }
export function prepareFile(path, text, expectedRevision) {
  return prepareRequest({kind: "write_file", data: {worktree: scope.worktree, path, text, expected_revision: expectedRevision}});
}

// Invocation-only typed state; no ADK objects or arbitrary session-state keys are exposed.
export function newId() { return call("id.new", null); }
export function callId() { return call("id.call", null); }

// Ephemeral package-owned policy; it never writes session history.
export function readTurnState() { return call("turn.read", null); }
export function stageTurnState(value) { return call("turn.stage", value); }
