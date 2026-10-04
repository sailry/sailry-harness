import {div} from 'gpui-kit';
import {Icon,Progress,Tab,TabBar,Pagination} from 'gpui-component';
import {theme,Header} from 'sailry';
import {EmptyState,SettingsGroup,DataTable,Heatmap,StackedChart,ProgressBar} from 'sailry/ui';
import {compact,total,integer,amount,speed,ranking} from './format.js';
import {heatmap,trend} from './charts.js';
import {requestRows} from './requests.js';

const column = () => div().v_flex().min_w_0().gap_3();
const label = text => div().text_sm().font_semibold().child(text);
const muted = text => div().text_sm().text_color(theme().colors.muted_foreground).child(text);
const card = (id,body) => div().id(id).min_w_0().child(SettingsGroup.new(`${id}-box`,{heading:false}).child(div().py_4().child(body)));

function summary(view,metrics) {
  const {text}=view,unknown=text.composer_metric_unknown,tokens=metrics.tokens;
  const values=[
    ['composer_tokens',total(tokens),'chart-pie'],['composer_input_tokens',tokens?.input,'arrow-down'],
    ['composer_output_tokens',tokens?.output,'arrow-up'],['composer_cached_tokens',tokens?.cached_input,'hard-drive'],
    ['composer_reasoning_tokens',tokens?.reasoning,'cpu'],['usage_responses',metrics.responses,'inbox'],
    ['usage_speed',speed(metrics.generation),'rotate-cw'],['usage_cost',amount(metrics.cost?.usd_micros),'chart-pie']
  ];
  const columns=window.viewport_size().width<1100?2:4,rows=[];
  for(let offset=0;offset<values.length;offset+=columns)rows.push(div().h_flex().gap_3().items_stretch()
    .children(values.slice(offset,offset+columns).map(([key,value,icon],index)=>{
      const raw=value==null?unknown:String(value),display=offset+index<6?compact(value)??unknown:raw;
      const partial=key==='usage_cost'&&metrics.cost&&integer(metrics.cost.responses)<integer(metrics.responses)?text.usage_cost_incomplete
        :key==='usage_speed'&&metrics.generation&&integer(metrics.generation.responses)<integer(metrics.responses)?text.usage_speed_partial:null;
      return div().flex_1().min_w_0().child(card(`usage-metric-${key}`,column().gap_2()
        .child(div().h_flex().gap_2().min_w_0().child(new Icon(`icons/${icon}.svg`).size('small').color(theme().colors[`chart_${(offset+index)%4+2}`]??theme().colors.chart_2))
          .child(muted(text[key]).truncate()))
        .child(div().id(`usage-value-${key}`).text_xl().font_semibold().truncate().tooltip(partial?`${raw} · ${partial}`:raw).child(display))));
    })));
  return column().id('usage-summary').children(rows);
}
function ranks(view,data) {
  const {text}=view,groups=data.groups.map(group=>({node:data.node,group}));
  const rows=ranking(groups),maximum=rows.reduce((maximum,row)=>row.tokens>maximum?row.tokens:maximum,1n);
  return column().id('usage-ranking').child(div().h_flex().justify_between().child(label(text.usage_model_ranking)).child(muted(text.composer_tokens)))
    .children(!rows.length?[EmptyState.new('usage-ranking-empty',{variant:'list',icon:'chart-pie',label:text.usage_empty})]:rows.map((row,index)=>{
      const {model,provider}=row.group.key.data;
      const name=view.inventory?.node===row.node?view.inventory.providers.find(item=>item.id===provider)?.name:null;
      const source=name??provider.slice(0,8);
      const exact=row.tokens==null?text.composer_metric_unknown:String(row.tokens),description=`${model} · ${source} · ${exact} ${text.composer_tokens}`;
      return div().id(`usage-rank-${index}`).h_flex().gap_4().py_1().min_w_0().tooltip(description)
        .child(div().v_flex().w('32%').min_w_0().gap_1().child(div().text_sm().truncate().child(model)).child(muted(source).text_xs().truncate()))
        .child(div().flex_1().min_w_0().children(row.tokens==null?[]:[ProgressBar.new(`usage-rank-bar-${index}`,{value:Number(row.tokens)/Number(maximum)*100,color:'chart_2',label:description})]))
        .child(muted(compact(row.tokens)??text.composer_metric_unknown).w_16().text_right());
    }));
}
function requests(view,data) {
  const {state,text}=view,rows=requestRows(data.requests.items,text);
  const fields=['model_provider','project_time','input_output','cached_reasoning','first_token_duration','cost'];
  const rowHeight=48;
  return column().child(div().id('usage-table').w_full().min_w_0().h(rowHeight+(rows.rows.length?rows.rows.length*rowHeight:96))
      .child(DataTable.new('usage-requests',{revision:`${state.generation}:${state.cursor}`,columns:fields.map(key=>text[`usage_column_${key}`]),
        ...rows,row_height:rowHeight,stripe:true,widths:[160,160,105,110,110,80],alignments:fields.map((_,index)=>index<2?'start':'end'),resizable:false,empty:text.usage_empty})))
    .children(state.page>1||data.requests.has_more?[div().id('usage-pagination').h_flex().justify_end()
      .child(new Pagination('usage-pages').size('small').current_page(state.page).total_pages(Math.max(state.cursors.length,state.page+Number(data.requests.has_more)))
        .disabled(!state.ready()).on_change((page,cx)=>view.page(page,cx)))]:[]);
}
export function render(view) {
  const {state,text}=view,data=state.data(),t=key=>text[key];
  const refreshing=state.view?.refreshing;
  const body=column().id('usage-content').w_full().max_w(800).mx_auto().p_6().gap_4()
    .children(!state.previous&&!view.error&&(refreshing||!data)?[new Progress('usage-loading').loading(true).size('xsmall').accessibility_label(text.usage_loading)]:[]);
  if(data)body.child(summary(view,data.totals))
    .child(card('usage-activity-card',column().child(div().h_flex().justify_between().child(label(text.usage_activity)).child(muted(text.usage_local_time)))
      .child(Heatmap.new('usage-heatmap-plot',heatmap(data.days,t)))))
    .child(card('usage-trend-card',column().child(div().h_flex().justify_between().child(label(text.usage_trend))
      .child(new TabBar('usage-period').variant('segmented').selected_index(state.period==='daily'?0:1).children([new Tab().label(text.usage_daily),new Tab().label(text.usage_weekly)])
        .on_change((index,cx)=>{state.period=index===0?'daily':'weekly';cx.notify();})))
      .child(div().h_flex().gap_3().children([['usage_column_cached',3],['usage_uncached_input',2],['usage_column_output',4]].map(([key,color])=>div().h_flex().gap_1()
        .child(div().w_2().h_2().rounded_full().bg(theme().colors[`chart_${color}`])).child(muted(text[key]).text_xs()))))
      .child(StackedChart.new(`usage-${state.period}-chart`,trend(data.days,state.period,t)))))
    .child(card('usage-ranking-card',ranks(view,data))).child(card('usage-requests-card',requests(view,data)));
  return div().id('usage-page').v_flex().size_full().min_w_0().min_h_0().child(Header.new('usage-header',{content:JSON.stringify(view.header())})).child(div().id('usage-scroll').flex_1().min_h_0().min_w_0().overflow_y_scroll().child(body));
}
