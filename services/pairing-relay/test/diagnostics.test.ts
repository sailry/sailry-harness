import { PagesBlobError, PreconditionFailedError, type Store } from "@edgeone/pages-blob";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BlobObjects } from "../src/blob";
import { report } from "../src/diagnostics";
import { serve } from "../src/edgeone";
import { handle } from "../src/http";

afterEach(() => vi.restoreAllMocks());
const PRIVATE = "private-ticket-123456-Bearer-credential-https://private.example/?token=secret";

describe("failure diagnostics", () => {
  it.each([
    ["CREDENTIAL_ERROR", `failed to obtain STS credential: 403 ${PRIVATE}`, { upstream_status: 403 }],
    ["COS_ERROR", `COS returned 503: ${PRIVATE}`, { upstream_status: 503 }],
    ["COS_ERROR", `COS returned 0: ${PRIVATE}`, { cause: "upstream_unknown" }],
    ["CREDENTIAL_ERROR", "invalid STS credential response", { cause: "invalid_sts_response" }],
    ["COS_ERROR", "COS returned 0: unable to derive tenant subdomain from credential; missing appId/zoneId/projectId or resourcePrefix", { cause: "invalid_sts_metadata" }],
  ])("classifies %s without payloads", (code, message, fields) => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    const error = new PagesBlobError(code as string, message as string);
    error.stack = PRIVATE;
    report("storage_list", error);
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", {
      stage: "storage_list", type: "PagesBlobError", code, ...fields as object,
    });
    expect(JSON.stringify(output.mock.calls)).not.toContain(PRIVATE);
  });

  it("ignores arbitrary names codes and messages", () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    report("body", Object.assign(new Error(PRIVATE), { name: PRIVATE, code: PRIVATE, stack: PRIVATE,
      cause: { code: PRIVATE, message: PRIVATE } }));
    report("request", PRIVATE);
    report("storage_init", new PagesBlobError(PRIVATE, PRIVATE));
    expect(output.mock.calls).toEqual([
      ["Pairing request failed", { stage: "body", type: "Error" }],
      ["Pairing request failed", { stage: "request", type: "string" }],
      ["Pairing request failed", { stage: "storage_init", type: "PagesBlobError" }],
    ]);
    expect(JSON.stringify(output.mock.calls)).not.toContain(PRIVATE);
  });

  it("accepts only anchored SDK status prefixes", () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    report("storage_get", new PagesBlobError("COS_ERROR", `${PRIVATE} COS returned 403:`));
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", {
      stage: "storage_get", type: "PagesBlobError", code: "COS_ERROR",
    });
  });

  it("retains an allowlisted network cause", () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    report("exchange", new TypeError(PRIVATE, { cause: { code: "ENOTFOUND", message: PRIVATE } }));
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", {
      stage: "exchange", type: "TypeError", cause: "ENOTFOUND",
    });
  });
});

function sdk() {
  return { get: vi.fn(async () => null), setJSON: vi.fn(async () => {}), delete: vi.fn(async () => {}),
    list: vi.fn(async () => ({ blobs: [] as { key: string; etag: string }[], directories: [] })) };
}
function post(body = "{}") {
  return new Request("https://pair.example/v1/codes", { method: "POST", body });
}

describe("failure boundaries", () => {
  it.each(["list", "delete"] as const)("reports %s before body validation once", async operation => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    const client = sdk();
    const error = new PagesBlobError("COS_ERROR", `COS returned 403: ${PRIVATE}`);
    if (operation === "list") client.list.mockRejectedValueOnce(error);
    else {
      client.list.mockResolvedValueOnce({ blobs: [{ key: "g/000000000000/expired", etag: "test" }], directories: [] });
      client.delete.mockRejectedValueOnce(error);
    }
    const response = await serve({ request: post() }, new BlobObjects(client as unknown as Pick<Store, "get" | "setJSON" | "list" | "delete">));
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "unavailable" });
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", {
      stage: operation === "list" ? "storage_list" : "storage_delete", type: "PagesBlobError", code: "COS_ERROR", upstream_status: 403,
    });
  });

  it("preserves write failures and quiet contention", async () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    const client = sdk();
    const store = new BlobObjects(client as unknown as Pick<Store, "get" | "setJSON" | "list" | "delete">);
    const error = new PagesBlobError("CREDENTIAL_ERROR", `failed to obtain STS credential: 401 ${PRIVATE}`);
    client.setJSON.mockRejectedValueOnce(error);
    await expect(store.create("key", { expires: 1, value: PRIVATE })).rejects.toBe(error);
    const response = await handle(post(), { execute: async () => { throw error; } }, "source");
    expect(response.status).toBe(503);
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", {
      stage: "storage_put", type: "PagesBlobError", code: "CREDENTIAL_ERROR", upstream_status: 401,
    });
    output.mockClear();
    client.setJSON.mockRejectedValueOnce(new PreconditionFailedError());
    expect(await store.create("key", { expires: 1, value: PRIVATE })).toBe(false);
    expect(output).not.toHaveBeenCalled();
  });

  it("distinguishes capacity from unexpected failures", async () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    const response = await handle(post(), { execute: () => ({ status: 503, body: { error: "capacity" } }) }, "source");
    expect(response.status).toBe(503);
    expect(await response.json()).toEqual({ error: "capacity" });
    expect(response.headers.get("retry-after")).toBe("60");
    expect(output).toHaveBeenCalledTimes(1);
    expect(output).toHaveBeenCalledWith("Pairing request failed", { stage: "exchange", type: "Capacity", code: "CAPACITY" });
  });

  it("keeps malformed JSON quiet", async () => {
    const output = vi.spyOn(console, "error").mockImplementation(() => {});
    expect((await handle(post("{"), { execute: vi.fn() }, "source")).status).toBe(400);
    expect(output).not.toHaveBeenCalled();
  });
});
