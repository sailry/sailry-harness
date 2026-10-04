/** Native Kit table. Page size is 50; scalar text is supplied exactly by the caller. */
export type TableLine={text:string;icon?:string;tone?:"primary"|"muted"|"chart_2"|"chart_3"|"chart_4"};
/** Presentation only: rows remain the exact scalar values for copy and dump. */
export type TableCell={primary:TableLine;secondary?:TableLine};
export const DataTable:{new(id:string,props:{revision:string;columns:string[];rows:string[][];cells?:TableCell[][];row_height?:number;page?:number;menu?:NativeMenuItem[];empty?:string;widths?:number[];alignments?:("start"|"end")[];details?:string[][];row_ids?:string[];resizable?:boolean;stripe?:boolean;empty_stripes?:boolean;header?:boolean}):import('gpui-kit').Element};
export function nextTableEvent():Promise<{table:string;revision:string;kind:'selection'|'action'|'copy';row:number|null;column:number|null;rows:number[];action?:string}>;
