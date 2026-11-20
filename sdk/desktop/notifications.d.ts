/** Information-only feedback scoped to this live view. */
export function toast(value: {
  id: string;
  message: string;
  kind: "info" | "error";
  /**
   * Why an error toast was shown: a protocol-style code such as
   * `unavailable`. An error toast is recorded as a failure under its id;
   * the code is reported with it and the message is not.
   */
  code?: string;
  /** @deprecated Legacy v1 metadata is accepted and ignored. */
  action?: {id: string; label: string};
}): void;
export function dismissToast(id: string): void;
/** @deprecated The legacy stream ends when the live view closes. */
export function nextToastEvent(): Promise<{id: string; action: string}>;
