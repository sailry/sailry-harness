export interface Document {
  id: string; path: string; revision: string | null; lines: number;
  position: { line: number; column: number }; language: string;
  dirty: boolean; saving: boolean; uncertain: boolean; truncated: boolean;
  mode: "source" | "document"; markdown: boolean; can_copy: boolean;
  can_edit: boolean; readonly: boolean; can_cut_paste: boolean; can_save: boolean;
  error: { code: string; message: string } | null;
}
export interface Documents {
  cursor: string; documents: Document[];
  operations: { id: string; action: { kind: "rename" | "copy"; from: string; to: string } | { kind: "trash" | "create_directory" | "write"; path: string }; running: boolean; uncertain: boolean }[];
  reveal: { document: string; sequence: string } | null;
}
export const DocumentSurface: { new(id: string, props: { document: string }): import("gpui-kit").Element };
export function readDocuments(): Documents;
export function refreshDocuments(): void;
export function nextDocumentChange(cursor: string): Promise<Documents>;
export function openDocument(path: string, options?: { line?: number }): Promise<Document>;
export function documentAction(id: string, action: { kind: "focus" | "source" | "document" | "copy" | "cut" | "paste" | "undo" | "redo" | "find" | "reveal"; line?: number }): Promise<Document>;
export function saveDocument(id: string): Promise<Document>;
export function closeDocument(id: string, options?: { discard?: boolean; confirm?: boolean }): Promise<boolean>;
