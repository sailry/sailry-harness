// The Node owns admission, scope checks and persistence; this module only adapts values.
const bridge = globalThis.__sailry;
delete globalThis.__sailry;
function call(operation, value) {
  const result = JSON.parse(bridge(operation, JSON.stringify(value)));
  if (result.Err) {
    const error = new Error(result.Err.message);
    error.code = result.Err.code;
    if (result.fault_id) error.sailryFault = result.fault_id;
    throw error;
  }
  return result.Ok;
}
const scope = call("context", null);
// Typed module peers reuse the same scoped bridge and draft owner.
export function modelCall(operation, value) { return call(operation, value); }
export function context() { return JSON.parse(JSON.stringify(scope)); }
export async function readProjectCatalog() { return read("read_project_catalog"); }
export async function listTeams() { return read("list_teams"); }
export async function listAssociatedSessions(association, before=null, limit=32) {
  return read("list_associated_sessions", {association:ownedAssociation(association),before,limit});
}
export async function readSession(session=scope.session) { return read("read_session", {session:session ?? scope.session}); }
export async function readConversation(before=null, limit=20, session=scope.session) { return read("read_conversation", {session:session ?? scope.session,before,limit}); }
export async function readConversationRuns(before=null,limit=100,session=scope.session) { return read("read_conversation_runs", {session:session ?? scope.session,before,limit}); }
export async function readConversationRun(turn,revision=null,session=scope.session) { return read("read_conversation_run", {session:session ?? scope.session,turn,expected_revision:revision}); }
export async function readTurn(turn,before=null,limit=100) {
  const history=await readConversation(null,1);
  return read("read_turn", {session:scope.session,turn,expected_revision:history.page.revision,before,limit});
}
function ownedAssociation(association) {
  if (!association || typeof association !== "object" || Array.isArray(association)) throw new Error("invalid session association");
  if (association.namespace !== undefined && association.namespace !== scope.package.name) throw Object.assign(new Error("session association belongs to another plugin"), {code:"permission_denied"});
  return {...association,namespace:scope.package.name};
}
export function startSession(draft) {
  const data = {...draft};
  if (data.association != null) data.association = ownedAssociation(data.association);
  return prepareRequest({kind:"start_session", data});
}
export function setSessionExecution(selection, revision, session=scope.session) {
  if (!selection || typeof selection !== "object" || Array.isArray(selection)
      || Object.keys(selection).some(key => !["execution","team","models"].includes(key))) throw new Error("invalid execution selection");
  return prepareRequest({kind:"set_session_team",data:{team:null,models:{},...selection,session:session ?? scope.session,expected_revision:revision}});
}
export function setSessionConfig(config, revision, session=scope.session) {
  return prepareRequest({kind:"set_session_config",data:{config,session:session ?? scope.session,expected_revision:revision}});
}
export function sendMessage(message, revision, session=scope.session) {
  return prepareRequest({kind:"queue_turn",data:{message,session:session ?? scope.session,expected_revision:revision}});
}
export function prepareRequest(command) { return call("prepare", command); }
export function prepareTransaction(operations) {
  return prepareRequest({kind: "plugin_transaction", data: {operations}});
}
/** Only the value owner's atomic key-CAS rejection is safe to replan. */
export function isValueConflict(outcome) {
  return outcome?.isError === true && outcome.error?.code === 'revision_conflict'
    && outcome.error.details?.kind === 'plugin_value_conflict';
}
export async function completeRequest(id) { return call("complete", id); }
export function forgetRequest(id) { call("forget", id); }
async function read(kind, data) {
  const budget = {list_plugin_keys:100,search_plugin_values:100,query_knowledge:100,read_knowledge_citation:65536}[kind];
  if (budget && data?.limit !== undefined) {
    if (!Number.isSafeInteger(data.limit) || data.limit < 1) throw new Error("limit must be a positive safe integer");
    // Bound only the output page; preserve the query and use the Node's exact continuation.
    data = {...data,limit:Math.min(data.limit,budget)};
  }
  return call("read", {kind, data}).data;
}
export async function requestProviderHttp(provider, revision, request, input={}) {
  return read("request_provider_http", {provider,expected_revision:revision,request,input});
}
export function prepareProviderHttp(provider, revision, request, input={}) {
  return prepareRequest({kind:"execute_provider_http",data:{provider,expected_revision:revision,request,input}});
}
export async function readSettings() { return read("read_plugin_settings", {package: scope.package}); }
/** Public catalog definitions only; fixed-model provider configuration remains private. */
export async function listRoles() { return read("list_roles"); }
export async function fetchWeb(input) { return read("fetch_web", input); }
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
/** Semantic collections reference raw text with index.source {key, pointer} in this namespace. */
export function indexedValue(key, value, index, expectedRevision) {
  return prepareRequest({kind: "write_indexed_plugin_value", data: {key, value, index, expected_revision: expectedRevision}});
}
/** Preserve the original query for semantic recall; query defaults to hybrid, terms alone to keyword. */
export async function searchValues(input) {
  const query = {...input};
  if (typeof query.query === "string" && query.query.trim() && query.mode === undefined) query.mode = "hybrid";
  return read("search_plugin_values", query);
}
export function deleteValue(key, expectedRevision) {
  return prepareRequest({kind: "remove_plugin_value", data: {key, expected_revision: expectedRevision}});
}
export function deleteConversationValue(key, expectedRevision) {
  return prepareRequest({kind: "remove_plugin_conversation_value", data: {key, expected_revision: expectedRevision}});
}
export function prepareNotification(content) {
  return prepareRequest({kind: "publish_notification", data: {package: scope.package, content}});
}
export async function queryKnowledge(query) { return read("query_knowledge", query); }
export async function listKnowledgeCollections() { return read("list_knowledge_collections"); }
/** Captures this worktree or the plugin's own storage; never accepts owner or sharing overrides. */
export function prepareKnowledgeCollection(draft) { return call("knowledge.create", draft); }
export function prepareKnowledgeRebuild(id, expectedRevision) {
  return prepareRequest({kind:"rebuild_knowledge_collection",data:{id,expected_revision:expectedRevision}});
}
export function prepareKnowledgeRemoval(id, expectedRevision) {
  return prepareRequest({kind:"delete_knowledge_collection",data:{id,expected_revision:expectedRevision}});
}
export async function inspectGit() { return read("inspect_git", {worktree: scope.worktree}); }
export async function readFile(path) { return read("read_file", {worktree: scope.worktree, path}); }
/** Zero-based line offset; the full-content revision is checked before slicing. */
export async function readFileLines(path, offset=0, limit=200, expectedRevision=null) {
  return read("read_file_lines", {worktree:scope.worktree,path,offset,limit,expected_revision:expectedRevision});
}
export async function searchCode(query={}) { return read("search_code", {worktree:scope.worktree,query}); }
export async function outlineCode(path) { return read("outline_code", {worktree:scope.worktree,path}); }
export function prepareFile(path, text, expectedRevision) {
  return prepareRequest({kind: "write_file", data: {worktree: scope.worktree, path, text, expected_revision: expectedRevision}});
}

// Invocation-only typed state; arbitrary session-state keys are not exposed.
export function newId() { return call("id.new", null); }
export function callId() { return call("id.call", null); }

// Ephemeral package-owned policy; it never writes session history.
export function readTurnState() { return call("turn.read", null); }
export function stageTurnState(value) { return call("turn.stage", value); }

export async function readComputerPermissions() { return read("read_computer_permissions"); }
export async function requestComputerPermission(permission) { return read("request_computer_permission", {permission}); }
