export type Kind = 'document'|'preview';
export type Content = {artifact:{id:string;session:string;kind:Kind;revision:number;title:string;summary:string};html:string;view:unknown;warnings:string[]};
export function artifactResource():{id:string;session:string;revision?:number}|null;
export function readArtifact(id:string,revision?:number):Promise<Content>;
export function listArtifactRevisions(id:string,before?:number):Promise<{items:{kind:Kind;revision:number;title:string;summary:string;created_at:number}[];next_before:number|null}>;
export function nextArtifactChange(cursor:string):Promise<{cursor:string;connected:boolean}>;
export function nextArtifactEvent():Promise<{kind:'edited';artifact:string;revision:number}>;
export const ArtifactSurface:{new(id:string,props:{artifact:string;revision?:number;labels?:Record<string,string>}):import('gpui-kit').Element};
/** Interactive previews support HTML export only. */
export function exportArtifact(options:{artifact:string;revision:number;format:'html'|'png'}):Promise<boolean>;
