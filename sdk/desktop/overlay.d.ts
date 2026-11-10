export const Tooltip: { new(id: string, props: { text: string }): import("gpui-kit").Element };
export const Modal: { new(id: string, props: { open: boolean; form?: boolean; title?: string; width?: number; dismissable?: boolean }): import("gpui-kit").Element };
/** Read-only Markdown details. Use as the body of Modal with form: true. */
export const Details: { new(id: string, props: { subtitle?: string; content: string }): import("gpui-kit").Element };
export function modal_closed(): Promise<string>;
