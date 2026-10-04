export type Profile = {id:string;revision:number;name:string;host:string;port:number;username:string;authentication:string;host_key:unknown;sharing?:unknown};
/** Native dialogs return opaque handles. No local paths or bytes enter JavaScript. */
export function selectUpload(profile:Profile,directory:string,prompt:string):Promise<string[]>;
export function selectDownload(profile:Profile,entries:{path:string;directory:boolean}[],prompt:string):Promise<string[]>;
/** Check observes the original publication receipt before continuing an admitted batch. */
export function transferAction(id:string,action:'start'|'replace'|'check'|'cancel'|'dismiss'):void;
export function nextTransfers(cursor:string):Promise<{cursor:string;transfers:{id:string;profile:string;kind:string;source:string;path:string;stage:string;error:unknown;progress:{copied:number;size:number}|null;can_start:boolean;can_replace:boolean;can_check:boolean;can_cancel:boolean}[]}>;

/** Consumes a one-use native tree drop handle in this mounted view. */
export function uploadDropped(profile:Profile,directory:string,handle:string):string[];
