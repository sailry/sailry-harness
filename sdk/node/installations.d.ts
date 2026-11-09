/** sailry/installations: only model requests admitted by this callback. */
export type Fault = {code:string; message:string};
export type Progress = {kind:'percent'; percent:number; bytes_per_second:number|null}
  | {kind:'counts'; completed:number; total:number; bytes_per_second:number|null};
export type State = {kind:'queued'} | {kind:'running'; phase:'downloading'|'inspecting'|'preparing'|'installing'|'validating'; progress:Progress|null}
  | {kind:'failed'|'uncertain'; error:Fault};
export type Task = {request:string; revision:string; target:'node'|'controller';
  resource:{kind:'model'; data:{kind:'ocr'|'embedding'|'speech'; id:string}}; state:State};
export function read(ids:string[]): Promise<Task[]>;
/** Live observation is unavailable in bounded callbacks; use read(ids). */
export function watch(ids:string[]): void;
export function next(): Promise<Task[]>;
/** Removes a terminal observation only; never cancels execution or clears durable outcomes. */
export function dismiss(id:string): Promise<void>;
