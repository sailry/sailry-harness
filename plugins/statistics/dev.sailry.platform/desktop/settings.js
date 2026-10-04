import {View} from 'gpui-kit';
import {context,header_action} from 'sailry';
import {readUsageInventory,readUsageChoices,watchUsage,nextUsageChange,refreshUsage} from 'sailry/sdk';
import {nextTableEvent,writeClipboard,toast} from 'sailry/ui';
import {requestRows} from './requests.js';
import {messages} from './locales.js';
import {State,query,choices} from './state.js';
import {render} from './view.js';

export default class Usage extends View {
  init(_props,cx) {
    const scope=JSON.parse(context());this.text=messages(scope.locale);this.state=new State(scope.node);
    this.inventory=null;this.resources=[];this.error=null;this.choiceCursor=null;
    this.observe(cx);
    cx.spawn(async ()=>{try {while(true) {
      const event=await nextTableEvent();
      if(event.table!=='usage-requests'||event.kind!=='copy'||event.revision!==`${this.state.generation}:${this.state.cursor}`)continue;
      const rows=requestRows(this.state.data()?.requests.items??[],this.text).rows;
      writeClipboard(event.rows.filter(index=>rows[index]).map(index=>rows[index].join('\t')).join('\n'));
    }} catch (_) { /* Releasing the table closes its event stream. */ }});
    cx.spawn(async cx=>{
      try {while(true) {
        const event=await header_action();
        if(event==='usage-refresh')this.refresh(cx);
        else {
          let value;try{value=JSON.parse(event);}catch(_){continue;}
          if(!value.id.startsWith('usage-filter-'))continue;
          const [generation,...parts]=value.value.split(':');
          if(generation!==String(this.state.generation))continue;
          const selection=parts.join(':'),field=value.id.slice('usage-filter-'.length);
          if(this.state.select(field,selection))this.observe(cx);
        }
      }} catch (_) { /* Closing the page releases its header. */ }
    });
  }
  observe(cx) {
    const generation=this.state.start();this.error=null;this.choiceCursor=null;
    try {watchUsage(this.state.query,false);}catch(_){this.report('usage_read_failed');cx.notify();return;}
    cx.notify();
    cx.spawn(async cx=>{
      try {while(generation===this.state.generation) {
        const event=await nextUsageChange(this.state.cursor);
        if(!this.state.accept(event,generation))continue;
        const current=query(this.state.days);
        if(current.start_ms!==this.state.query.start_ms || current.end_ms!==this.state.query.end_ms) {
          Object.assign(this.state.query,{start_ms:current.start_ms,end_ms:current.end_ms});
          this.state.reset();this.state.view=null;this.observe(cx);return;
        }
        const error=this.state.view?.error?.code==='invalid_request' ? 'usage_query_failed' : this.state.view?.error ? 'usage_read_failed' : null;
        if(error)this.report(error);else this.error=null;
        const report=this.state.view?.report;
        if(report && this.choiceCursor!==report.cursor) {this.choiceCursor=report.cursor;this.loadChoices(generation,cx);}
        cx.notify();
      }} catch (_) { if(generation===this.state.generation){this.report('usage_read_failed');cx.notify();} }
    });
  }
  report(key) { if(this.error!==key)toast({id:'usage-error',message:this.text[key],kind:'error'});this.error=key; }
  loadChoices(generation,cx) {
    const query={...this.state.query,providers:[],models:[],before:null},cursor=this.choiceCursor;
    cx.spawn(async cx=>{
      try {
        const [inventory,report]=await Promise.all([readUsageInventory(this.state.node),readUsageChoices(query,this.state.node)]);
        if(generation!==this.state.generation || cursor!==this.choiceCursor)return;
        this.inventory=inventory;this.resources=report.resources;cx.notify();
      } catch (_) { /* Existing resource labels remain usable when a refresh fails. */ }
    });
  }
  refresh(cx) {
    const current=query(this.state.days);
    if(current.start_ms!==this.state.query.start_ms || current.end_ms!==this.state.query.end_ms || this.state.page!==1 || this.error) {
      Object.assign(this.state.query,{start_ms:current.start_ms,end_ms:current.end_ms});this.state.reset();this.state.view=null;this.observe(cx);
    } else refreshUsage();
  }
  page(value,cx) {if(this.state.turn(value))this.observe(cx);}
  header() {
    const state=this.state,text=this.text,options=choices(state,this.inventory,this.resources,text);
    const field=(name,label,items,selected,primary=false)=>({id:`usage-filter-${name}`,label,items:items.map(item=>({...item,id:`${state.generation}:${item.id}`})),selected:`${state.generation}:${selected}`,primary});
    return {title:text.usage,filters_label:text.usage_filters,filters:[
      field('range',text.usage_range,[7,30,90,365].map(days=>({id:String(days),label:text.usage_range_days.replace('%{days}',days)})),String(state.days),true),
      ...['project','provider','model'].map(name=>field(name,text[`usage_all_${name}s`],options[name],state.query[`${name}s`][0] ?? ''))],
      actions:[{id:'usage-refresh',label:text.usage_refresh,icon:'icons/rotate-cw.svg',disabled:state.view?.refreshing ?? false}]};
  }
  render() {return render(this);}
}
