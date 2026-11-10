export function listDatabases():Promise<unknown[]>;
export function listSsh():Promise<unknown[]>;
export function listSshTerminals(profile:string):Promise<import('sailry/sdk').TerminalInfo[]>;
export function prepareSshTerminal(profile:string,revision:number):string;
export function newDatabaseId():string;
export function newSshId():string;
/** Secret IDs come from this mounted view's native credential drafts. */
export function prepareDatabase(input:{profile:unknown;secret?:string;testing:boolean}):string;
export function prepareSsh(input:{profile:unknown;credential?:{kind:'password'|'private_key';secret:string;passphrase?:string}}):string;
export function browseDatabase(profile:string,revision:number,database?:string):Promise<unknown>;
export function browseSshDirectory(input:{profile:string;expected_revision:number;path:string;after?:unknown}):Promise<unknown>;
/** Observe the original request only. Does not submit or replay it. */
export function requestOutcome(id:string):Promise<{kind:'not_admitted'|'admitted'|'unknown'|'completed';data?:unknown}>;
/** Database integer cell values and affected_rows are exact decimal strings in controller results. */
