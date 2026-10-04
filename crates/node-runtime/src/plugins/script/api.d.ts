/** Node callback subset of sailry/sdk. UI and model APIs are not installed here. */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Entry = { key: string; revision: string; value: Json; present: boolean };
/** Opaque private state follows visible conversation history. Restoration does not authorize execution. */
export type ConversationEntry = Entry & { restored: boolean };
/** Turn initialization receives the admitted configuration, never live session defaults.
 * config is null unless the package declares conversation.read. */
export type TurnInput = { project: string | null; tools: string[]; mode: "plan" | "code"; config: Json };
export function context(): Json;
/** Registered projects and worktrees only; requires projects.read and an unscoped context. */
export function readProjectCatalog(): Promise<Json>;
/** Atomically creates and queues the first turn. Requires sessions.start; null config uses Node defaults. */
export function startSession(draft: {project: string | null; worktree: string | null; config: Json; title: string; message: {text: string; attachments: string[]; references?: Json[]}}): string;
export function prepareRequest(command: Json): string;
/** Up to 64 private KV, dispatch, notification or scoped turn operations, committed atomically. */
export function prepareTransaction(operations: Json[]): string;
export function completeRequest(id: string): Promise<Json>;
export function forgetRequest(id: string): void;
export function readSettings(): Promise<{ values: Record<string, Json>; ready: boolean }>;
export function getValue(key: string): Promise<Entry>;
export function getConversationValue(key: string): Promise<ConversationEntry>;
export function listKeys(prefix?: string, after?: string | null, limit?: number): Promise<{ keys: string[]; after: string | null }>;
export function setValue(key: string, value: Json, expectedRevision: string): string;
export function deleteValue(key: string, expectedRevision: string): string;
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
export function readConversation(before?:string|null,limit?:number): Promise<Json>;
export function readTurn(turn:string,before?:number|null,limit?:number): Promise<Json>;
/** Ephemeral policy state shared only by this package's callbacks and tools in one turn. */
export function readTurnState(): Json;
export function stageTurnState(value:Json): Json;
