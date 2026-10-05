export interface BrowserTab { id: number; title: string; url: string; loading: boolean; loaded: boolean; error: string | null }
export interface BrowserState { cursor: string; tabs: BrowserTab[]; selected: number | null; history: [boolean,boolean]; supported: boolean }
export function readBrowser(): BrowserState;
export function nextBrowserChange(seen: string): Promise<BrowserState>;
export function browserAction(action: {kind: 'add'|'select'|'close'|'navigate'|'back'|'forward'|'reload'|'stop'; id?: number; url?: string}): Promise<BrowserState>;
export function readBrowserSettings(): {supported:boolean; enabled:boolean; persistent:boolean};
export function setBrowserPersistent(value:boolean): void;
export function listBrowserProfiles(): Promise<{id:string;name:string}[] | null>;
export function importBrowserProfile(id:string): Promise<{count:number;skipped:number} | null>;
