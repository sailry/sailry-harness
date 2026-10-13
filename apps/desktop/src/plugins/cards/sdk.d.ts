export interface PublicProps { [key: string]: string | number | boolean | null }
export interface State {
  cursor: string; props: PublicProps; configured: boolean; can_open: boolean; can_cancel: boolean;
  login: 'idle'|'starting'|'pending'|'exchanging'|'connected'|'cancelled'|'failed'|'unconfirmed';
  locale?: string; action: string | null;
}
export type Mode = 'embedded' | 'external';
export type ActionIntent = {kind:'connect';target?:string;mode?:Mode} | {kind:'cancel'}
  | {kind:'refresh'} | {kind:'settings'} | {kind:'browser';target:string;mode:Mode};
export function state(): State;
export function theme(): {is_dark:boolean;colors:Record<string,string>};
/** Own declared connector metadata only; no URLs, credentials or token contents. */
export function status(): Promise<{
  package:{name:string;digest:string;settings_revision:number}; card:string; server:string;
  configuration:'ready'|'missing'|'changed'|'unsupported';
  authorization:'not_declared'|'missing'|'configured';
  credential:'not_declared'|'absent'|'present'; configured:boolean;
}>;
export function nextChange(cursor:string): Promise<State>;
/** Ephemeral display state; never updates the receipt or canonical history. */
export function publishProps(props:PublicProps):void;
export function toast(message:string,error?:boolean):void;
/** Kit Button with a native, user-click-only capability binding. */
export const Action:{new(id:string,props:{action:ActionIntent;label:string;disabled?:boolean;primary?:boolean}):import('gpui-kit').Element};
/** Reads only the declared bounded QR property and expiry from this instance. */
export const Qr:{new(id:string,props?:{}):import('gpui-kit').Element};
