export interface BrowserTab { id: number; title: string; url: string; loading: boolean; loaded: boolean; error: string | null }
export interface BrowserElement { url:string;selector:string;text:string;bounds:[number,number,number,number];viewport:[number,number];truncated:boolean }
export interface BrowserAnnotation {token:string;element:BrowserElement}
export interface BrowserState { cursor: string; tabs: BrowserTab[]; selected: number | null; history: [boolean,boolean]; supported: boolean; annotating: boolean; annotation: BrowserAnnotation | null; annotation_error: string | null }
export function readBrowser(): BrowserState;
export function nextBrowserChange(seen: string): Promise<BrowserState>;
export function browserAction(action: {kind: 'add'|'select'|'close'|'navigate'|'back'|'forward'|'reload'|'stop'|'annotate'|'cancel_annotation'; id?: number; url?: string; token?: string}): Promise<BrowserState>;
/** Validate the captured token/reference and transfer its private screenshot to the captured composer. */
export function submitBrowserAnnotation(token:string, reference:string): Promise<void>;
export function readBrowserSettings(): {supported:boolean; enabled:boolean; persistent:boolean};
export function setBrowserPersistent(value:boolean): void;
export function listBrowserProfiles(): Promise<{id:string;name:string}[] | null>;
export function importBrowserProfile(id:string): Promise<{count:number;skipped:number} | null>;
