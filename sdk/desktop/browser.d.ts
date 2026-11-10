export interface BrowserTab { id: number; title: string; url: string; loading: boolean; loaded: boolean; error: string | null }
export interface BrowserComponent { framework:'react'|'vue'|'svelte'|'angular';name:string;source:string|null;stack:string[] }
/** Untrusted page content; never carries input values. */
export interface BrowserElement { url:string;title:string;selector:string;path:string;tag:string;id:string;classes:string[];attributes:Record<string,string>;name:string;text:string;html:string;styles:Record<string,string>;component:BrowserComponent|null;bounds:[number,number,number,number];viewport:[number,number];truncated:boolean }
export interface BrowserAnnotation {token:string;element:BrowserElement}
export interface BrowserState { cursor: string; tabs: BrowserTab[]; selected: number | null; history: [boolean,boolean]; supported: boolean; annotating: boolean; annotation: BrowserAnnotation | null; annotation_error: string | null }
export function readBrowser(): BrowserState;
export function nextBrowserChange(seen: string): Promise<BrowserState>;
export function nextBrowserShortcut(): Promise<{kind: 'add'}>;
export function browserAction(action: {kind: 'add'|'select'|'close'|'navigate'|'back'|'forward'|'reload'|'stop'|'annotate'|'cancel_annotation'; id?: number; url?: string; token?: string}): Promise<BrowserState>;
/** Validate the reviewed token/reference and insert its element chip and note into the captured composer. */
export function submitBrowserAnnotation(token:string, reference:string): Promise<void>;
export function readBrowserSettings(): {supported:boolean; enabled:boolean; persistent:boolean};
export function setBrowserPersistent(value:boolean): void;
export function listBrowserProfiles(): Promise<{id:string;name:string}[] | null>;
export function importBrowserProfile(id:string): Promise<{count:number;skipped:number} | null>;
