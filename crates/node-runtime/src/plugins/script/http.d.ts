/** Pure helpers for declared http.request operation flows; these never send requests. */
export function text(value:unknown,name:string):string;
export function encoded(value:string):string;
export function id(value:unknown,name?:string):string;
export function query(values:Record<string,unknown>):string;
export function limit(value?:number,maximum?:number):{requested_limit:number;page_size:number};
export function failure(message:string,status?:number|null):{isError:true;error:{code:'unavailable';message:string};status:number|null};
export function response(outcome:unknown):{value?:Record<string,unknown>;error?:unknown};
export function call(url:string,connector:string,state?:unknown,options?:{method?:string;headers?:Record<string,string>;body?:string|null;connection?:string;idempotent?:boolean}):{call:{operation:'http.request';arguments:unknown};state:unknown};
