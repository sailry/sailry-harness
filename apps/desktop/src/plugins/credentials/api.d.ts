/** Opaque view-local native input. Secret contents never become JavaScript values. */
export function createSecret(options:{label:string;placeholder?:string;multiline?:boolean}):string;
export function releaseSecret(id:string):void;
export function describeSecret(id:string):{filled:boolean;file:string|null;picking:boolean;version:number};
export function clearSecret(id:string):void;
/** Editing notifications carry metadata only, never the credential contents. */
export function nextSecretEvent():Promise<{id:string;kind:'change'|'blur'|'enter'|'focus';version:number;filled:boolean}>;
/** Imports a controller-selected UTF-8 key file; returns its display name only. */
export function chooseSecretFile(id:string,prompt:string):Promise<string|null>;
export const SecretField:{new(id:string,props?:{disabled?:boolean}):import('gpui-kit').Element};
