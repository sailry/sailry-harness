export type ActivityHost = {node: string; label: string};
export type ActivityHosts = {hosts: ActivityHost[]; unread_terminals: string[]; visible: boolean};
export type ActivityCatalog = {
  node: string; cursor: string; projects: unknown[]; worktrees: unknown[];
  sessions: unknown[]; terminals: unknown[]; session_lanes: Record<string,string>;
  terminal_lanes: Record<string,string>; hosts: ActivityHost[]; unread_terminals: string[];
};
/** Read the captured Node's canonical activity resources through its declared activity.read action. */
export function readActivityCatalog(): Promise<ActivityCatalog>;
/** Read up to eight previews from the same authorized Node. */
export function readActivityPreviews(sessions: string[]): Promise<{session: string; turn: string | null; text: string | null}[]>;
/** Observe controller labels and unread terminal IDs without starting another task subscription. */
export function nextActivityHosts(seen: ActivityHosts): Promise<ActivityHosts>;
