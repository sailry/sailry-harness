export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Model = { model: string; kind: "provider"; effort?: Json };
export type Turn = { instructions: string; state: Json; choices: ({ id: number } & Record<string, Json>)[] };
export type Entry = { key: string; revision: string; value: Json; present: boolean };
/** Immutable resource and package binding for this view. */
export function context(): { package: {name: string; digest: string; settings_revision: string}; worktree: string | null; session: string | null; turn: string | null; invocation: string | null };
/** Creates a fresh identifier without admitting an operation. */
export function newId(): string;
export function errorCode(message: string): string;
/** Classifies a structured Node fault carried by a native method rejection. */
export function faultCode(message: string): string;
export type Settings = { package: {name: string; digest: string; settings_revision: string}; values: Record<string, Json>; configured: string[]; keepable: string[]; ready: boolean };
export function readSettings(): Promise<Settings>;
export type ComputerPermissions = {
  node: string; platform: string; local: boolean;
  screen_capture: boolean | null; accessibility: boolean | null;
};
/** Reads the captured execution Node without requesting OS permission. */
export function readComputerPermissions(): Promise<ComputerPermissions>;
/** Explicit permission guidance is allowed only on the execution device's local client. */
export function requestComputerPermission(permission: "screen_capture" | "accessibility"): Promise<ComputerPermissions>;
export function listModels(): Promise<{ models: Json[] }>;
/** Read-only resolution on the captured execution Node; credentials remain opaque references. */
export function resolveSessionModel(model:string, effort?:Json|null, config?:Json|null):Promise<Json>;
export function resolveModel(settings: Json, catalog: Json, field: string): Model;
export function modelCommand(player: Model, turn: Turn): Json;
export function modelChoice(result: Json, player: Model, moves: Json[]): { move: Json; tokens: number };
export function prepareModel(player: Model, turn: Turn): string;
export function prepareRequest(command: Json): string;
/** Up to 64 private KV, dispatch mutation, or notification operations, committed atomically. */
export function prepareTransaction(operations: Json[]): string;
export type NotificationDraft = {
  title: string; message: string; kind: "info" | "success" | "warning" | "error";
  session?: string | null;
};
/** Requires notifications.publish; completeRequest submits the durable request. */
export function prepareNotification(content: NotificationDraft): string;
export function completeRequest(id: string): Promise<Json>;
export function forgetRequest(id: string): void;
/** Coalesced Node and cached panel entry invalidation, without resource data.
 * Pass the returned opaque cursor to wait; one pending call per view. Reuses Client recovery.
 * Entry changes on cached panel navigation, independently of Node events. */
export function nextChange(cursor?: string): Promise<{cursor: string; connected: boolean; entry: string}>;
export function getValue(key: string): Promise<Entry>;
export function listKeys(prefix?: string, after?: string | null, limit?: number): Promise<{ keys: string[]; after: string | null }>;
/** Prepare a durable compare-and-set write; completeRequest returns its receipt. */
export function setValue(key: string, value: Json, expectedRevision: string): string;
export type ValueIndex = { fields: [string, string]; tags: string[]; order: number };
export function indexedValue(key: string, value: Json, index: ValueIndex, expectedRevision: string): string;
/** Literal OR terms; filters apply before paging, using this package's whole indexed corpus. */
export function searchValues(query: { terms: string[]; weights?: [number, number]; all?: string[]; any?: string[]; offset?: number; limit?: number }): Promise<{ entries: Entry[]; next: number | null; now_ms: number }>;
export function deleteValue(key: string, expectedRevision: string): string;
export type LocalizedLabel = { label: string; locales?: Record<string, string> };
export type ContributionChoice = {
  id: string; label: LocalizedLabel; description?: LocalizedLabel;
  icon?: string; group?: LocalizedLabel; enabled?: boolean;
};
export type ContributionState = {
  id: string; enabled?: boolean; visible?: boolean; label?: LocalizedLabel; icon?: string; dropdown?: boolean; value?: string | number | boolean | null;
  choices?: ContributionChoice[]; details?: { label: LocalizedLabel; value: string }[];
  reply_to?: number;
};
export type ContributionEvent = {
  sequence: number; id: string; handler: string; kind: "invoke" | "change" | "search";
  value: string | number | boolean | null;
};
export function publishContributions(states: ContributionState[]): void;
export function nextContributionEvent(): Promise<ContributionEvent>;
/** The captured session only; credentials remain on its execution Node. */
export function readSession(): Promise<{ id: string; revision: string; config: Json } & Record<string, Json>>;
export function readConversation(before?: string | null, limit?: number): Promise<Json>;
/** Prepare durable commands, then use completeRequest with the returned ID. */
export function sendMessage(input: Json, expectedRevision: string): string;
export function stopTurn(turn: string): string;
export function setSessionConfig(config: Json, expectedRevision: string): string;
/** Pass the previous cursor; one pending call per view. Uses shared Client recovery. */
export function nextConversation(cursor?: string): Promise<{ cursor: string; view: Json }>;
export type HttpRequest = {
  method: "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS";
  url: string; headers?: Record<string, string>; body?: string;
  credential?: string; timeout_ms?: number;
};
export type HttpResponse = { status: number; headers: Record<string, string[]>; body: string };
/** Prepare a Node HTTP request. Complete its original ID; redirects/retries are disabled. */
export function requestHttp(request: HttpRequest): string;
export type GitChange = "added" | "modified" | "deleted" | "renamed" | "type_changed";
export type GitEntry = {
  path: string; staged: GitChange | null; unstaged: GitChange | null;
  untracked: boolean; conflicted: boolean;
  diff: { additions: number; deletions: number };
  staged_diff: { additions: number; deletions: number };
  unstaged_diff: { additions: number; deletions: number };
};
export type GitStatus = {
  kind: "directory" | "unborn" | "ready";
  branch: string | null; head: string | null; index_revision: string | null;
  entries: GitEntry[]; truncated: boolean; omitted_paths: number;
};
export type GitIndexDraft = {
  operation: "stage" | "unstage"; paths: string[];
  expected_index: string; expected_head: string | null;
};
export type GitCommitOptions = {
  signoff: boolean; skip_hooks: boolean; tracked: boolean; all: boolean;
  after: "none" | "push" | "sync"; push_remote: string | null;
};
export type GitCommitDraft = {
  message: string; amend?: boolean; options?: GitCommitOptions;
  expected_index: string; expected_head: string | null; expected_branch: string | null;
};
/** Requires git.read; reads only the worktree captured by this instance. */
export function inspectGit(): Promise<GitStatus>;
/** Requires git.write. Complete the returned request ID; never silently refresh revisions. */
export function prepareGitIndex(draft: GitIndexDraft): string;
/** Requires git.write. Reuse the original ID to recover an uncertain commit result. */
export function prepareGitCommit(draft: GitCommitDraft): string;
export type GitLogCursor = {head: string; offset: number};
export type GitDiff = {path: string; scope: "all" | "staged" | "unstaged"; text: string; additions: number; deletions: number; binary: boolean; truncated: boolean};
/** All repository reads retain this instance's captured worktree. */
export function listGitBranches(): Promise<Json>;
export function resolveGitRevision(revision: string): Promise<Json>;
export function readGitDiff(path: string, scope: "all" | "staged" | "unstaged"): Promise<GitDiff>;
export function readGitLog(limit: number, cursor?: GitLogCursor | null): Promise<Json>;
export function readGitCommit(commit: string): Promise<Json>;
export function readGitStash(commit: string): Promise<GitDiff>;
export function readGitOutput(): Promise<{text: string}>;
export function listGitRemoteTags(remote: string): Promise<Json[]>;
/** Closed Git branch/action commands. The Node validates displayed revisions and durable admission. */
export function prepareGitChange(draft: Json): string;
/** Registered resources in this captured worktree's project; requires worktrees.read. */
export function readWorktreeCatalog(): Promise<Json>;
/** Native Git checkout discovery for this captured repository. */
export function listWorktrees(): Promise<Json>;
/** Register, create, managed-create or remove, with the source captured by this instance. */
export function prepareWorktreeChange(draft: Json): string;
/** Registered projects and worktrees only; requires projects.read and an unscoped context. */
export function readProjectCatalog(): Promise<Json>;
/** Atomically creates and queues the first turn. Requires sessions.start; null config uses Node defaults. */
export function startSession(draft: {project: string | null; worktree: string | null; config: Json; title: string; message: {text: string; attachments: string[]; references?: Json[]}}): string;

export type TerminalTool = "codex" | "claude" | "gemini" | "agy" | "grok" | "opencode" | "kimi";
export type TerminalInfo = {id: string; revision: number; worktree: string | null; title: string | null; directory: string | null; tool: TerminalTool | null; ssh: string | null; status: {kind: "running" | "closed" | "stopped"} | {kind:"exited"; data:{code:number}} | {kind:"failed"; data:{message:string}}};
/** Reads only the worktree captured by this view; requires terminals.read. */
export function listTerminals(): Promise<TerminalInfo[]>;
export function listTerminalTools(): Promise<{tool: TerminalTool; available: boolean}[]>;
/** Requires terminals.control. Submit with completeRequest; reuse the original ID on an uncertain outcome. */
export function prepareTerminal(tool?: TerminalTool | null): string;
export function prepareOpenTerminal(terminal: string): string;
export function prepareCloseTerminal(terminal: string): string;

export type MediaKind = "vision" | "image" | "video";
export type MediaBinding = {provider: string; model: string};
export type MediaSettings = {revision: number; bindings: Partial<Record<MediaKind, MediaBinding>>};
/** Settings surface only; credentials and provider endpoints remain on the execution Node. */
export function readMediaSettings(): Promise<MediaSettings>;
export function listMediaModels(): Promise<{provider: string; provider_name: string; model: string; kinds: MediaKind[]}[]>;
/** Captures the supplied revision; complete the original request ID to recover an uncertain save. */
export function prepareMediaSettings(settings: MediaSettings): string;

export type DirectoryCursor = {revision: string; directory: boolean; name: string};
export function listDirectory(path: string, after?: DirectoryCursor | null): Promise<{path: string; entries: {name: string; kind: "file" | "directory" | "symlink" | "other"; size: number}[]; truncated: boolean; unsupported_names: number; revision: string; next: DirectoryCursor | null}>;
export function searchFiles(options: {query: string; regex: boolean; case_sensitive: boolean; globs: string[]}): Promise<{matches: {path: string; line_number: number; line: string}[]; scanned_files: number; skipped: number; truncated: boolean}>;
export function cancelFileSearch(): void;
export function prepareFileAction(action: {kind: "write"; path: string; text: string; revision: string | null} | {kind: "create_directory" | "trash"; path: string} | {kind: "rename" | "copy"; from: string; to: string}): string;
/** Node-owned role settings use the original request on uncertain retries. */
export function listRoles(): Promise<Array<unknown>>;
export function newRoleId(): string;
export function prepareRole(role: unknown, expected_revision: number): string;
export function prepareRemoveRole(id: string, expected_revision: number): string;
/** Canonical Sailry reports only. Integer counters and cursors are exact decimal strings. */
export interface UsageQuery {start_ms:number|string;end_ms:number|string;dimension:'model';projects:string[];worktrees:string[];providers:string[];models:string[];before:null|{node:string;timestamp_ms:string;sequence:string}}
export function usageSources():{node:string;label:string}[];
export function readUsageInventory(node?:string):Promise<{node:string;projects:{id:string;name:string}[];providers:{id:string;name:string;models:string[]}[]}>;
export function readUsageChoices(query:UsageQuery,node?:string):Promise<Record<string,unknown>>;
export function watchUsage(query:UsageQuery,all:boolean,node?:string):void;
export function nextUsageChange(cursor:string):Promise<{cursor:string;all:boolean;view:Record<string,unknown>|null}>;
export function refreshUsage():void;

export function nextUsageSources(seen:{node:string;label:string}[]):Promise<{node:string;label:string}[]>;
