/** Information-only feedback scoped to this live view. */
export function toast(value: {
  id: string;
  message: string;
  kind: "info" | "error";
  /**
   * Why an error toast was shown: a protocol-style code such as
   * `unavailable`. The error toast is logged under its id and code, not its message.
   */
  code?: string;
  /** Pass through the same Node fault's `report_ref` and `code`; the view verifies the pair against its received faults. */
  report_ref?: string;
  /** @deprecated Legacy v1 metadata is accepted and ignored. */
  action?: {id: string; label: string};
}): void;
export function dismissToast(id: string): void;
/** @deprecated The legacy stream ends when the live view closes. */
export function nextToastEvent(): Promise<{id: string; action: string}>;
