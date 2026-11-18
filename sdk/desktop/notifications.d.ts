/** Information-only feedback scoped to this live view. */
export function toast(value: {
  id: string;
  message: string;
  kind: "info" | "error";
  /** @deprecated Legacy v1 metadata is accepted and ignored. */
  action?: {id: string; label: string};
}): void;
export function dismissToast(id: string): void;
/** @deprecated The legacy stream ends when the live view closes. */
export function nextToastEvent(): Promise<{id: string; action: string}>;
