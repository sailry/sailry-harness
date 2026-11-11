/** Node callback subset of sailry/sdk. Model installation and card modules have separate declarations. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Execution = "model" | "team";
/** Omit revision to resolve the current team on fresh admission. */
export type TeamSelection = {id: string; revision?: string | null};
export type MemberModel = {provider: string; model: string; effort?: Json};
export type ExecutionSelection = {
  execution: Execution; team?: TeamSelection | null; models?: Record<string, MemberModel>;
};
export type SessionAssociation = {kind: string; id: string; namespace?: string};
export type Session = {id: string; revision: string; config: Json} & Record<string, Json>;
export type TeamProfile = {
  id: string; revision: string; key: string; name: string; description: string;
  appearance: Json; model: MemberModel | null; instructions: string; members: string[];
};
/** Requires teams.read. Catalog revisions remain exact decimal strings. */
export function listTeams(): Promise<TeamProfile[]>;
/** Requires conversation.read; namespace is the calling package's name. */
export function listAssociatedSessions(association: SessionAssociation, before?: string | null, limit?: number): Promise<{sessions: Session[]; next_before: string | null}>;
/** Requires conversation.control and the captured or package-associated session. */
export function setSessionExecution(selection: ExecutionSelection, expectedRevision: string, session?: string | null): string;
export type Entry = { key: string; revision: string; value: Json; present: boolean };
/** Opaque private state follows visible conversation history. Restoration does not authorize execution. */
export type ConversationEntry = Entry & { restored: boolean };
/** Turn initialization receives the admitted configuration, never live session defaults.
 * config is null unless the package declares conversation.read. message is the user's text,
 * capped at 4 KiB UTF-8, and null unless the package declares conversation.message.read. */
export type TurnInput = {
  project: string | null; tools: string[]; mode: "plan" | "code"; config: Json; options: Record<string, Json>; session_revision: string | null; message: string | null;
  /** True when no user input started the turn: a notice turn or a package continuation. */
  automatic: boolean; notice: boolean;
  /** Present only for this package's own continuation; marker is the declared marker id it chose, or null. */
  continuation?: {marker: string | null};
};
export type TurnTokens = { input: number; output: number; cached_input: number; reasoning: number };
/** Why continue_turn was refused, in the fault's details ({kind: "continuation", data}). count and limit accompany "limit". */
export type ContinuationRefusal = {
  reason: "plan" | "child" | "archived" | "worktree" | "queue_paused" | "queued_input" | "running" | "interaction" | "limit" | "stale";
  count?: number; limit?: number;
};
/** host.turn.after_turn input for an ended main task turn. status is completed, failed, cancelled (the user's
 * Stop) or interrupted (the Node lost the turn); error is the stored fault code of a turn that did not complete.
 * tools counts the model's tool calls by the name in names or the core tool name; expired counts approvals and
 * questions that timed out unanswered. usage covers this turn and the children it delegated, session_usage every
 * turn of the session. background.running is work that starts a notice turn when it ends; background.pending is
 * ended results no turn has taken. chain is the run of automatic turns since the last user input and its limit. */
export type TurnEnd = {
  turn: string; status: "completed" | "failed" | "cancelled" | "interrupted"; error?: string;
  mode: "plan" | "code"; automatic: boolean; notice: boolean; continuation?: {marker: string | null};
  names: string[]; tool_calls: number; tools: Record<string, number>; expired: number;
  usage: TurnTokens; session_usage: TurnTokens;
  background: {running: number; pending: number};
  chain: {count: number; limit: number};
};
/** Turn initialization result. instruction is per-turn context text. parts are separate context pieces,
 * at most 8 nonempty strings and 32 KiB UTF-8 in total. The Node adds a part only when no agent-visible
 * context of the conversation already holds that exact text, so unchanged parts are not repeated. */
export type TurnOutput = { tools: string[]; instruction: string; parts?: string[]; parameters?: Record<string, Json> };
/** host.status.read receives {} in the captured Composer scope, with null turn/invocation.
 * Return StatusState[] for this package's declared Status ids; omitted ids become hidden.
 * The Node owns these states and rejects duplicates, invalid ids, more than 32 entries,
 * or more than 64 KiB of serialized state. Desktop cannot publish Status ids.
 * Labels are localized data. Keep value null and omit scope, choices and reply_to. */
export type StatusState = {
  id: string;
  label?: {label: string; locales?: Record<string, string>} | null;
  icon?: string | null;
  visible?: boolean;
  enabled?: boolean;
  loading?: boolean;
  value?: null;
  segments?: {
    label: {label: string; locales?: Record<string, string>};
    tone: "accent" | "muted" | "success" | "warning" | "danger";
  }[];
  /** Values are at most 4096 UTF-8 bytes; progress is a finite percentage in [0, 100]. */
  details?: {label: {label: string; locales?: Record<string, string>}; value: string; progress?: number | null}[];
};
export function context(): Json;
/** Registered projects and worktrees only; requires projects.read and an unscoped context. */
export function readProjectCatalog(): Promise<Json>;
/** Atomically creates and queues the first turn. Requires sessions.start; null config uses Node defaults. */
export function startSession(draft: {
  project: string | null; worktree: string | null; config: Json; title: string;
  message: {text: string; attachments: string[]; references?: Json[]};
  execution?: Execution; team?: TeamSelection | null; models?: Record<string, MemberModel>;
  association?: SessionAssociation | null;
}): string;
export function prepareRequest(command: Json): string;
/** Up to 64 private KV, dispatch, notification or scoped queue/turn operations, committed atomically. */
export function prepareTransaction(operations: Json[]): string;
/** A known zero-write key-CAS rejection, excluding caller/configuration fences and uncertainty. */
export function isValueConflict(outcome: Json): boolean;
export function completeRequest(id: string): Promise<Json>;
export function forgetRequest(id: string): void;
export function readSettings(): Promise<{ values: Record<string, Json>; ready: boolean }>;
/** Safe execution-Node catalog, not session snapshots or provider configuration. */
export type RoleProfile = {
  id:string; revision:string; key:string; name:string; description:string; instructions:string;
  read_only:boolean; appearance:Json; model:{provider:string;model:string;effort:Json}|null;
  max_turns:number|null; skills:string[];
};
export function listRoles(): Promise<RoleProfile[]>;
export type WebRequest = { url: string; offset?: number };
export type WebPage = {
  url: string; status: number; content_type: string; content: string; title: string | null;
  next_offset: number | null; truncated: boolean; untrusted: boolean;
};
/** Public, credential-free reads require web.read. Offsets count Unicode characters in the cached page. */
export function fetchWeb(input: WebRequest): Promise<WebPage>;
export type ProviderHttpResponse = {status:number;headers:Record<string,string[]>;body:string};
/** Only named read declarations are available without durable admission. */
/** Reads an exact declared provider resource; native origins and private headers remain Node-owned. */
export function requestProviderHttp(provider:string,revision:string,request:string,input?:Record<string,string>):Promise<ProviderHttpResponse>;
/** Complete the stable request ID with completeRequest; retry that ID after uncertain outcomes. */
export function prepareProviderHttp(provider:string,revision:string,request:string,input?:Record<string,string>):string;
export function getValue(key: string): Promise<Entry>;
export function getConversationValue(key: string): Promise<ConversationEntry>;
export function listKeys(prefix?: string, after?: string | null, limit?: number): Promise<{ keys: string[]; after: string | null }>;
export function setValue(key: string, value: Json, expectedRevision: string): string;
export function deleteValue(key: string, expectedRevision: string): string;
export type KnowledgeMode = "keyword" | "semantic" | "hybrid";
export type KnowledgeDraft = {
  name: string;
  source: {kind: "worktree"; path: string} | {kind: "storage"; scope: "node" | "conversation"; prefix: string};
  model?: string | null; globs?: string[]; tags?: string[];
};
export type KnowledgeCollection = {
  id: string; revision: string; owner: {kind: "controller"} | {kind: "plugin"; name: string};
  definition: Json; progress: Json;
};
export type KnowledgeQuery = {
  query: string; collections?: string[]; mode?: KnowledgeMode; tags?: string[]; path?: string | null;
  modified_after_ms?: number | null; modified_before_ms?: number | null; limit?: number; offset?: number;
};
export type KnowledgePosition =
  {kind: "lines"; start: number; end: number} | {kind: "page"; page: number} |
  {kind: "cells"; sheet: string; start: string; end: string} | {kind: "slide"; slide: number} |
  {kind: "section"; name: string} | {kind: "value"; key: string; pointer: string};
export type KnowledgePage = {
  hits: {title: string; text: string; score: number; stale: boolean;
    citation: {collection: string; chunk: string; revision: string; path: string; position: KnowledgePosition}}[];
  next_offset: number | null; mode: KnowledgeMode; indexing: boolean; model: string | null; untrusted: boolean;
};
/** Read-only local retrieval. Original query bytes and source citations are preserved; text remains untrusted. */
export function queryKnowledge(query: KnowledgeQuery): Promise<KnowledgePage>;
export function listKnowledgeCollections(): Promise<KnowledgeCollection[]>;
/** Captures the current worktree or this plugin's storage. Does not download a model or grant sharing. */
export function prepareKnowledgeCollection(draft: KnowledgeDraft): string;
/** Use the exact revision from listKnowledgeCollections or a completed collection receipt. */
export function prepareKnowledgeRebuild(id: string, expectedRevision: string): string;
/** Removes derived indexes and the collection definition; original source data is retained. */
export function prepareKnowledgeRemoval(id: string, expectedRevision: string): string;

export function setConversationValue(key: string, value: Json, expectedRevision: string): string;
export function deleteConversationValue(key: string, expectedRevision: string): string;
export function prepareNotification(content: {
  title: string; message: string; kind: "info" | "success" | "warning" | "error";
  session?: string | null;
}): string;
export function inspectGit(): Promise<Json>;
export function readFile(path: string): Promise<{ text: string; revision: string | null; truncated: boolean }>;
/** Zero-based offsets, one-based returned line numbers; requires files.read. */
export function readFileLines(path:string, offset?:number, limit?:number, expectedRevision?:string|null):Promise<FileLines>;
export type FileLines = {path:string; lines:{number:number; text:string}[]; size:number; revision:string; offset:number; next_offset:number|null; truncated:boolean};
/** Zero-based positions and UTF-8 byte columns; end_byte is exclusive. */
export type CodeRange = {start_byte:number; end_byte:number; start_line:number; start_column:number; end_line:number; end_column:number};
export type CodeSymbol = {path:string; language:string; name:string; kind:string; range:CodeRange; signature:string|null; file_revision:string; depth:number};
export type CodeQuery = {query?:string; name?:string|null; path?:string|null; language?:string|null; limit?:number};
export type CodeStatus = "parsed"|"unsupported"|"too_large"|"invalid"|"failed";
export type CodeSearch = {symbols:CodeSymbol[]; index_revision:number; partial:boolean; truncated:boolean; visited:number; skipped:number; issues:{path:string;status:CodeStatus}[]};
export type CodeOutline = {path:string; language:string|null; file_revision:string|null; index_revision:number; status:CodeStatus; symbols:CodeSymbol[]; capabilities:{structure:boolean;symbols:boolean;data:boolean;embedded:boolean}; partial:boolean; truncated:boolean};
/** Literal symbol search through the current worktree; requires code.index.read. */
export function searchCode(query?:CodeQuery):Promise<CodeSearch>;
/** Parses only this file when it is absent from the index; requires code.index.read. */
export function outlineCode(path:string):Promise<CodeOutline>;
export function prepareFile(path: string, text: string, expectedRevision: string | null): string;

/** Creates an identifier without admission. */
export function newId(): string;
/** Stable for this turn, package, tool and canonical call across flow evaluations. */
export function callId(): string;
export function readSession(session?: string | null): Promise<Session>;
/** page.queue.revision is an exact decimal string for queue_pause transactions. */
export function readConversation(before?:string|null,limit?:number,session?:string|null): Promise<Json>;
/** Lifecycle metadata only. Structural and queue revisions are exact decimal strings. */
export function readConversationRuns(before?: string | null, limit?: number, session?: string | null): Promise<Json>;
export function readConversationRun(turn: string, revision?: string | null, session?: string | null): Promise<Json>;
/** Prepare durable commands, then complete the returned stable request ID. */
export function sendMessage(input: Json, expectedRevision: string, session?: string | null): string;
export function setSessionConfig(config: Json, expectedRevision: string, session?: string | null): string;
export function readTurn(turn:string,before?:number|null,limit?:number): Promise<Json>;
/** Ephemeral policy state shared only by this package's callbacks and tools in one turn. */
export function readTurnState(): Json;
export function stageTurnState(value:Json): Json;

export type ComputerPermissions = {
  node: number[];
  platform: string;
  local: boolean;
  screen_capture: boolean | null;
  accessibility: boolean | null;
};
export function readComputerPermissions(): Promise<ComputerPermissions>;
/** Explicit local onboarding; the Node rejects remote requests and read-only callbacks. */
export function requestComputerPermission(permission: "screen_capture" | "accessibility"): Promise<ComputerPermissions>;
