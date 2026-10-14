export interface ChartRow { label:string; value:string }
export interface HeatmapProps {
  rows:number;
  columns:number;
  cells:Array<{column:number; row:number; intensity:number; label:string; label_group?:string;
    tooltip:{heading:string; title:string; rows:ChartRow[]}}>;
}
/** Cells scale with the available width (pitch at most 24px) so every column stays visible. */
export const Heatmap:{new(id:string, props:HeatmapProps):import("gpui-kit").Element};
export interface StackedChartProps {
  series:Array<{id:string; color:'chart_1'|'chart_2'|'chart_3'|'chart_4'|'chart_5'}>;
  /** Empty labels leave a bucket unlabeled; known all-zero buckets show a baseline mark. */
  buckets:Array<{label:string; values:number[]|null;
    tooltip:{heading:string; rows:ChartRow[]; footer:ChartRow}}>;
  maximum:number;
  ticks:Array<{value:number; label:string}>;
  /** Element height from 96 to 480, default 148; bars are at most 28px wide. */
  height?:number;
}
export const StackedChart:{new(id:string, props:StackedChartProps):import("gpui-kit").Element};
/** Native Kit Progress with a semantic chart color. */
export const ProgressBar:{new(id:string,props:{value:number;color?:'chart_1'|'chart_2'|'chart_3'|'chart_4'|'chart_5';label:string}):import('gpui-kit').Element};
