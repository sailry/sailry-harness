/** Native Kit table. Page size is 50; scalar text is supplied exactly by the caller. */
export type TableTone="primary"|"muted"|"warning"|"chart_2"|"chart_3"|"chart_4";
/** One cell line: optional leading monogram badge (or `badge_icon` on the same surface) and icon (colored by `tone`), the text (colored by `text_tone`), a muted trailing note, a bordered chip and a proportion meter (value 0-100, width 16-240, default 36; `fill` stretches it across the remaining line). */
export type TableLine={text:string;icon?:string;tone?:TableTone;text_tone?:TableTone;badge?:string;badge_icon?:string;note?:string;chip?:string;meter?:{value:number;label:string;tone?:TableTone;icon?:string;width?:number;fill?:boolean}};
/** Presentation only: rows remain the exact scalar values for copy and dump. */
export type TableCell={primary:TableLine;secondary?:TableLine};
export const DataTable:{new(id:string,props:{revision:string;columns:string[];rows:string[][];cells?:TableCell[][];row_height?:number;header_height?:number;page?:number;menu?:NativeMenuItem[];empty?:string;empty_icon?:string;widths?:number[];alignments?:("start"|"end")[];details?:string[][];row_ids?:string[];resizable?:boolean;stripe?:boolean;header?:boolean;/** Column that absorbs the remaining table width; its `widths` entry is the minimum. */fill?:number}):import('gpui-kit').Element};
export function nextTableEvent():Promise<{table:string;revision:string;kind:'selection'|'action'|'copy';row:number|null;column:number|null;rows:number[];action?:string}>;
