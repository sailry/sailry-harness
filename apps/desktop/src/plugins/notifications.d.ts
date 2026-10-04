/** Controller feedback, scoped to this live view; it does not create a Node notification. */
export function toast(value: {id: string; message: string; kind: "info" | "error"; action?: {id: string; label: string}}): void;
export function dismissToast(id: string): void;
export function nextToastEvent(): Promise<{id: string; action: string}>;
