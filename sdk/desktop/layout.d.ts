export const PanelHeader: { new(id: string, props?: { padding?: number; gap?: number; surface?: "content"; size?: "row"; draggable?: boolean; bordered?: boolean }): import("gpui-kit").Element };
/** List and card share content; fill_height controls either variant independently. */
export const EmptyState: { new(id: string, props: { icon: string; label: string; variant?: "panel" | "list" | "card"; fill_height?: boolean; vertical_align?: "start" | "center" }): import("gpui-kit").Element };
/** Optional plain body precedes rows in the same scroll region. Body and row text are bounded to 32768 UTF-8 bytes. Children form a fixed, right-aligned footer. */
export const StatusList: { new(id: string, props: { items: {id:string;text:string;state:"pending"|"in_progress"|"completed"|"skipped"|"paused"|"blocked"|"failed";details?:boolean;animated?:boolean}[]; body?: string; empty: string }): import("gpui-kit").Element };
export const Loading: { new(id: string): import("gpui-kit").Element };
export const InputSurface: { new(id: string): import("gpui-kit").Element };
/** With header_action, the first child is the trailing header control. */
export const SettingsGroup: { new(id: string, props: { title?: string; heading?: boolean; header_action?: boolean; variant?: "fill" | "outline" }): import("gpui-kit").Element };
/** Native application settings row; label is already localized and children form its trailing control. */
export const SettingsRow: { new(id: string, props: { label: string }): import("gpui-kit").Element };
/** The shared centered 800px page, with a scrolling body and arbitrary children. */
export const SettingsPage: { new(id: string, props: { title: string; description?: string }): import("gpui-kit").Element };
/** Children are rows. Loading opts into a first-read card; existing rows survive refresh. */
export const CardList: { new(id: string, props?: { loaded?: boolean; loading?: boolean; empty_icon?: string; empty_label?: string }): import("gpui-kit").Element };
/** Non-selectable Kit card; children are CardColumn elements with their own controls. */
export const CardRow: { new(id: string, props?: { row_id?: string }): import("gpui-kit").Element };
/** Shared column widths and alignment; content and callbacks remain with the caller. */
export const CardColumn: { new(id: string, props?: { variant?: "leading" | "title" | "metadata" | "wide_metadata" | "control" | "content" | "inline" | "actions"; spacing?: "compact" | "regular" }): import("gpui-kit").Element };
/** Flexible two-line title/subtitle; children are optional leading controls. */
export const CardSummary: { new(id: string, props: { title: string; subtitle: string; icon?: string }): import("gpui-kit").Element };
/** Host-owned expansion. Exactly one header and content slot, and at most one actions slot. */
export const Collapse: { new(id: string, props: { label: string; default_open?: boolean; variant?: "panel" | "plain" }): import("gpui-kit").Element };
/** Arbitrary Kit children; slot placement is independent of construction order. */
export const CollapseSlot: { new(id: string, props: { variant: "header" | "content" | "actions" }): import("gpui-kit").Element };
/** Current host choice, or null before this group is materialized. */
export function collapseOpen(id: string): boolean | null;
export const ScrollRegion: { new(id: string, props: { max_height: number }): import('gpui-kit').Element };
/** One native text line with shaped baselines. reference_size aligns separate lines sharing a parent row. */
export const TextLine: { new(id: string, props: { runs: { text: string; size: number; weight: number; color: string; gap?: number }[]; line_height: number; reference_size?: number | null }): import("gpui-kit").Element };
