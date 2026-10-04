export interface ChartRow { label:string; value:string }
export interface HeatmapProps {
  rows:number;
  columns:number;
  cells:Array<{column:number; row:number; intensity:number; label:string; label_group?:string;
    tooltip:{heading:string; title:string; rows:ChartRow[]}}>;
}
export const Heatmap:{new(id:string, props:HeatmapProps):import("gpui-kit").Element};
export interface StackedChartProps {
  series:Array<{id:string; color:'chart_1'|'chart_2'|'chart_3'|'chart_4'|'chart_5'}>;
  buckets:Array<{label:string; values:number[]|null;
    tooltip:{heading:string; rows:ChartRow[]; footer:ChartRow}}>;
  maximum:number;
  ticks:Array<{value:number; label:string}>;
}
export const StackedChart:{new(id:string, props:StackedChartProps):import("gpui-kit").Element};
/** Native Kit Progress with a semantic chart color. */
export const ProgressBar:{new(id:string,props:{value:number;color?:'chart_1'|'chart_2'|'chart_3'|'chart_4'|'chart_5';label:string}):import('gpui-kit').Element};
