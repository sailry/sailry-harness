/** Node callback subset of sailry/sdk. Model installation and card modules have separate declarations. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Entry = { key: string; revision: string; value: Json; present: boolean };
/** Opaque private state follows visible conversation history. Restoration does not authorize execution. */
export type ConversationEntry = Entry & { restored: boolean };
/** Turn initialization receives the admitted configuration, never live session defaults.
 * config is null unless the package declares conversation.read. */
export type TurnInput = { project: string | null; tools: string[]; mode: "plan" | "code"; config: Json; options: Record<string, Json>; session_revision: string | null };
export function context(): Json;
/** Registered projects and worktrees only; requires projects.read and an unscoped context. */
export function readProjectCatalog(): Promise<Json>;
/** Atomically creates and queues the first turn. Requires sessions.start; null config uses Node defaults. */
export function startSession(draft: {project: string | null; worktree: string | null; config: Json; title: string; message: {text: string; attachments: string[]; references?: Json[]}}): string;
export function prepareRequest(command: Json): string;
/** Up to 64 private KV, dispatch, notification or scoped queue/turn operations, committed atomically. */
export function prepareTransaction(operations: Json[]): string;
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
export function getValue(key: string): Promise<Entry>;
export function getConversationValue(key: string): Promise<ConversationEntry>;
export function listKeys(prefix?: string, after?: string | null, limit?: number): Promise<{ keys: string[]; after: string | null }>;
export function setValue(key: string, value: Json, expectedRevision: string): string;
export function deleteValue(key: string, expectedRevision: string): string;
export type KnowledgeModel = "granite-multi" | "bge-small-zh" | "potion-multi" | {registered: {id: string}};
export type KnowledgeMode = "keyword" | "semantic" | "hybrid";
export type KnowledgeDraft = {
  name: string;
  source: {kind: "worktree"; path: string} | {kind: "storage"; scope: "node" | "conversation"; prefix: string};
  model?: KnowledgeModel | null; globs?: string[]; tags?: string[];
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
  next_offset: number | null; mode: KnowledgeMode; indexing: boolean; model: KnowledgeModel | null; untrusted: boolean;
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
export function prepareFile(path: string, text: string, expectedRevision: string | null): string;

/** Creates an identifier without admission. */
export function newId(): string;
/** Stable for this turn, package, tool and canonical call across flow evaluations. */
export function callId(): string;
export function readSession(): Promise<Json>;
/** page.queue.revision is an exact decimal string for queue_pause transactions. */
export function readConversation(before?:string|null,limit?:number): Promise<Json>;
export function readTurn(turn:string,before?:number|null,limit?:number): Promise<Json>;
/** Ephemeral policy state shared only by this package's callbacks and tools in one turn. */
export function readTurnState(): Json;
export function stageTurnState(value:Json): Json;
