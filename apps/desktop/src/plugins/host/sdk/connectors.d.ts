export type Kind = 'secret'|'oauth2'|'application'|'mcp_oauth'|'local_cli';
export interface Connection {
  id:string;connector:string;method:Kind;revision:string;enabled:boolean;
  state:'connected'|'not_connected'|'connecting'|'expired'|'needs_confirmation'|'failed';
  package:{name:string;digest:string;settings_revision:string};
  identity:{name:string;id:string}|null;expires_at_ms:number|null;checked_at_ms:number|null;
  fields:Record<string,string>;configured:string[];
  consent?:{changes:({kind:"origins"|"scopes"|"targets";previous:string[];current:string[]}|{kind:"client"|"flow";previous:string;current:string}|{kind:"mcp";servers:string[]}|{kind:"source"})[]};
}
/** Only the invoking package's public state; credentials stay on the Node. */
export function list():Promise<Connection[]>;
export function status(connector:string):Promise<Connection|null>;
export function types():Promise<unknown[]>;
/** Prepare a browser login. Complete the original request ID and retain it after an uncertain result. */
export function login(input:{connector:string;connection?:string|null;expected_revision?:string|null;method:'oauth2'|'mcp_oauth';redirect?:string|null}):string;
export function cancel(attempt:string):string;
