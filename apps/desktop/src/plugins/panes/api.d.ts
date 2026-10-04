export type PaneResource = { kind: "terminal"; id: string };
/** This view's captured workspace resource, or null in a launcher/settings view. */
export function pane(): { resource: PaneResource } | null;
/** Open this package's ordinary page in the shared Kit workspace. Returns false while the resource is unavailable in the captured Node/worktree snapshot. */
export function openPane(view: { resource: PaneResource; title: string }): boolean;
export function updatePane(view: { title?: string; busy?: boolean }): void;
/** Replaces this view's transient resource titles within its captured worktree. */
export function publishResourceTitles(titles: { resource: PaneResource; title: string }[]): void;
/** Releases only this pane's view. Complete any explicit resource closure through the SDK first. */
export function closePane(): void;
/** A standalone pane's close action is delivered to its package; split detach stays in the host. */
export function nextPaneEvent(): Promise<{ kind: "close" }>;
