export interface Document {
  id: string; path: string; revision: string | null; lines: number;
  position: { line: number; column: number }; language: string;
  dirty: boolean; saving: boolean; uncertain: boolean; truncated: boolean;
  mode: "source" | "document"; markdown: boolean; notebook: boolean; can_copy: boolean;
  can_edit: boolean; readonly: boolean; can_cut_paste: boolean; can_save: boolean;
  error: { code: string; message: string } | null;
  /** Soft wrap of the source editor. */
  wrap: boolean;
  /** Markdown caret formatting; `block` is null for tables, images and other atomic blocks. */
  format: { bold: boolean; italic: boolean; strike: boolean; code: boolean; link: boolean;
    block: DocumentBlock | null } | null;
}
export type DocumentBlock = "paragraph" | "heading1" | "heading2" | "heading3" | "heading4" | "heading5" | "heading6" | "bullet" | "ordered" | "task" | "quote" | "callout" | "code";
export type DocumentInsertion = "table" | "image" | "rule" | "code" | "mermaid" | "math" | "chart" | "html";
export type DocumentAction =
  | { kind: "focus" | "source" | "document" | "copy" | "cut" | "paste" | "undo" | "redo" | "find" | "replace" | "wrap" | "bold" | "italic" | "strike" | "inline_code" | "link" }
  | { kind: "reveal"; line: number }
  | { kind: "block"; block: DocumentBlock }
  | { kind: "insert"; block: DocumentInsertion };
export interface Documents {
  cursor: string; documents: Document[];
  operations: { id: string; action: { kind: "rename" | "copy"; from: string; to: string } | { kind: "trash" | "create_directory" | "write"; path: string }; running: boolean; uncertain: boolean }[];
  reveal: { document: string; sequence: string } | null;
}
export const DocumentSurface: { new(id: string, props: { document: string }): import("gpui-kit").Element };
export function readDocuments(): Documents;
export function refreshDocuments(): void;
export function nextDocumentChange(cursor: string): Promise<Documents>;
/** Obsolete reads reject with the structured `cancelled` Fault code. */
export function openDocument(path: string, options?: { line?: number }): Promise<Document>;
export function documentAction(id: string, action: DocumentAction): Promise<Document>;
export function saveDocument(id: string): Promise<Document>;
export function closeDocument(id: string, options?: { discard?: boolean; confirm?: boolean }): Promise<boolean>;
