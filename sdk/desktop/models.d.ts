export type Target = "node" | "controller";
export type Kind = "ocr" | "embedding" | "speech" | "decision";
export type Hub = "hugging_face" | "model_scope";
export type Source = { hub: Hub; repository: string; revision: string };
export type Asset = { source: Source; path: string; name: string; bytes: number; sha256: string };
export type Format =
  | { kind: "transformer"; dimension: number; pooling: "cls" | "mean"; token_types: boolean }
  | { kind: "static"; dimension: number }
  | { kind: "sense_voice" }
  | { kind: "decision" }
  | { kind: "paddle_ocr"; family: "v5" | "v6"; orientation: boolean };
export type Definition = {
  source: Source; name: string; license: string; format: Format;
  query_prefix?: string | null; document_prefix?: string | null; assets: Asset[];
};
export type ModelRef =
  | { target: "node"; kind: "ocr" | "embedding" | "decision"; id: string }
  | { target: "controller"; kind: "speech"; id: string | null };
export type Search = { kind: Kind; hub: Hub; query: string; cursor: string | null };
export type Entry = { source: Source; name: string; format: Format };
export type Page = { items: Entry[]; next_cursor: string | null; partial: boolean };
export type Version = { revision: string; name: string };
export type Model = { id: string; definition: Definition; state: { kind: "missing" | "installed" | "downloading" } };
export function list(input: { target: "node"; kind?: "ocr" | "embedding" | "decision" }): Promise<Model[]>;
export function search(input: Search): Promise<Page>;
export function versions(input: { hub: Hub; repository: string }): Promise<Version[]>;
export function resolve(input: { kind: Kind; source: Source }): Promise<Definition>;
/** Capture registration only; completeRequest returns the registered identity. */
export function register(input: { target: Target; definition: Definition }): string;
/** Capture installation of an existing identity; completeRequest dispatches it. */
export function prepare(model: ModelRef): string;
