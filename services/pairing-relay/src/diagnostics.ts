import { PagesBlobError } from "@edgeone/pages-blob";

type Stage = "storage_init" | "storage_list" | "storage_get" | "storage_put" | "storage_delete"
  | "cleanup" | "request" | "body" | "exchange";
const stages = new WeakMap<object, Stage>();
const codes = new Set(["INVALID_KEY", "INVALID_STORE_NAME", "MISSING_ENVIRONMENT", "MISSING_PROJECT_ID",
  "QUOTA_EXCEEDED", "RATE_LIMITED", "CREDENTIAL_ERROR", "COS_ERROR", "PRECONDITION_FAILED"]);
const causes = new Set(["ENOTFOUND", "EAI_AGAIN", "ECONNREFUSED", "ECONNRESET", "ETIMEDOUT",
  "UND_ERR_CONNECT_TIMEOUT", "UND_ERR_HEADERS_TIMEOUT", "UND_ERR_SOCKET", "ABORT_ERR",
  "ERR_TLS_CERT_ALTNAME_INVALID", "CERT_HAS_EXPIRED", "DEPTH_ZERO_SELF_SIGNED_CERT", "UNABLE_TO_VERIFY_LEAF_SIGNATURE"]);
const names = new Set(["Error", "TypeError", "RangeError", "SyntaxError", "AbortError", "TimeoutError"]);

/** Retain the failing operation without changing the thrown object or logging twice. */
export function failed(stage: Stage, error: unknown): never {
  if (typeof error === "object" && error !== null && !stages.has(error)) stages.set(error, stage);
  throw error;
}

/** Only closed classifications and SDK-owned numeric status prefixes leave this owner. */
export function report(stage: Stage, error: unknown): void {
  const object = typeof error === "object" && error !== null ? error : undefined;
  const type = error instanceof PagesBlobError ? "PagesBlobError"
    : error instanceof Error ? names.has(error.name) ? error.name : "Error" : typeof error;
  const detail: { stage: Stage; type: string; code?: string; upstream_status?: number; cause?: string } = {
    stage: object && stages.get(object) || stage, type,
  };
  if (error instanceof PagesBlobError) {
    if (codes.has(error.code)) detail.code = error.code;
    const status = error.code === "COS_ERROR" ? /^PagesBlob: COS returned (\d{1,3}):/.exec(error.message)
      : error.code === "CREDENTIAL_ERROR" ? /^PagesBlob: failed to obtain STS credential: (\d{3})(?:\s|$)/.exec(error.message) : null;
    if (status && Number(status[1]) >= 100 && Number(status[1]) <= 599) detail.upstream_status = Number(status[1]);
    if (status?.[1] === "0") detail.cause = "upstream_unknown";
    if (error.code === "COS_ERROR" && error.message.startsWith("PagesBlob: COS returned 0: unable to derive tenant subdomain from credential;"))
      detail.cause = "invalid_sts_metadata";
    if (error.code === "CREDENTIAL_ERROR") {
      if (error.message === "PagesBlob: invalid STS credential response") detail.cause = "invalid_sts_response";
      else if (error.message === "PagesBlob: storage quota exceeded") detail.cause = "quota";
      else if (error.message === "PagesBlob: rate limited, please retry later") detail.cause = "rate_limit";
      else if (error.message.startsWith("PagesBlob: failed to obtain STS credential: fetch failed")) detail.cause = "transport";
    }
  }
  const cause = object && "cause" in object ? object.cause : undefined;
  const code = cause && typeof cause === "object" && "code" in cause ? cause.code
    : object && "code" in object ? object.code : undefined;
  if (typeof code === "string" && causes.has(code)) detail.cause = code;
  console.error("Pairing request failed", detail);
}

export function capacity(): void {
  console.error("Pairing request failed", { stage: "exchange", type: "Capacity", code: "CAPACITY" });
}
