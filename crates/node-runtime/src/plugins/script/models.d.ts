/** sailry/models: public discovery plus separately completed registration and installation. */
import type {KnowledgeModel} from './api';
export type Kind = 'ocr' | 'embedding' | 'speech';
export type Hub = 'hugging_face' | 'model_scope';
export type Source = {hub:Hub; repository:string; revision:string};
export type Asset = {source:Source; path:string; name:string; bytes:number; sha256:string};
export type Format = {kind:'transformer'; dimension:number; pooling:'cls'|'mean'; token_types:boolean}
  | {kind:'static'; dimension:number} | {kind:'sense_voice'}
  | {kind:'paddle_ocr'; family:'v5'|'v6'; orientation:boolean};
export type Definition = {source:Source; name:string; license:string; format:Format; assets:Asset[];
  query_prefix?:string|null; document_prefix?:string|null};
export type ModelRef = {target:'node'; kind:'ocr'; id:string}
  | {target:'node'; kind:'embedding'; model:KnowledgeModel}
  | {target:'controller'; kind:'speech'; id:string|null};
export type Search = {kind:Kind; hub:Hub; query:string; cursor:string|null};
export type Page = {items:{source:Source; name:string; format:Format}[]; next_cursor:string|null; partial:boolean};
export function search(input:Search): Promise<Page>;
export function versions(input:{hub:Hub; repository:string}): Promise<{name:string; revision:string}[]>;
export function resolve(input:{kind:Kind; source:Source}): Promise<Definition>;
/** Requires models.install; returns an ordinary prepared ID, not an installed model. */
export function register(input:{target:'node'|'controller'; definition:Definition}): string;
/** Requires models.install; headless controller targets fail with unavailable. */
export function prepare(input:ModelRef): string;
