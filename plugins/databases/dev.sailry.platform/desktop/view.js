import {div} from 'gpui-kit';
import {Button,Icon,Pagination,Tab,TabBar,Resizable,ResizablePanel} from 'gpui-component';
import {Conversation,Header,theme} from 'sailry';
import {Workspace,PanelHeader,EmptyState,Loading,InputSurface,Picker,SelectableRow,IconButton,Tooltip,ResourceTree,DataTable,Modal,NativeContextMenu} from 'sailry/ui';
import {TextField,readText} from 'sailry/forms';
import {command,cell} from './main.js';
import {format} from './locales.js';
import {editor,editorFooter} from './editor-view.js';

function description(profile,text) {
  const connection=profile.connection;
  if(connection.kind==='sqlite')return `SQLite · ${connection.path}`;
  if(connection.kind==='socket')return `${text.db_transport_socket} · ${connection.path}`;
  return `${text[connection.engine==='mysql'?'db_mysql':'db_postgres']} · ${connection.username}@${connection.host}:${connection.port}`;
}
function picker(view) {
  const {text}=view,profile=view.profiles.find(profile=>profile.id===view.pickerProfile),disabled=view.locked();
  const actions=[['open','connection_action_open','arrow-right'],['edit','connection_action_edit','settings-2'],['check','connection_action_check','check'],['remove','settings_delete','delete']];
  const items=profile?[...actions.map(([action,label,icon])=>({id:`connection-action-${action}`,label:text[label],icon,disabled,value:{kind:'action',action}})),
    {id:'connection-back',label:text.reference_back,icon:'chevron-left',value:{kind:'back'}}]:[
    ...view.profiles.map((profile,index)=>({id:`connection-choice-${index}`,label:profile.name,detail:description(profile,text),icon:'hard-drive',submenu:true,disabled,value:{kind:'profile',id:profile.id}})),
    {id:'connection-add',label:text.db_new,icon:'plus',disabled,value:{kind:'add'}}];
  return Picker.new(`db-picker-${view.pickerRevision}`,{mode:'command',open:view.picker,title:text.connection_search,empty:text.connection_search_empty,items});
}
function landing(view) {
  const {text}=view;
  return EmptyState.new('db_workspace_empty',{icon:'hard-drive',label:text.db_workspace_empty})
    .child(div().w_full().max_w_96().child(IconButton.new('db-new',{icon:'plus',label:text.db_new,show_label:true,full_width:true,variant:'primary',size:'medium',disabled:view.locked()})))
    .child(div().id('db-saved-connections').v_flex().w_full().max_w_96().max_h(320).overflow_y_scroll().gap_2()
      .children(view.profiles.map(profile=>SelectableRow.new(`db-open-${profile.id}`,{variant:'card',label:profile.name,disabled:view.locked(),selected:false})
        .child(div().h_flex().w_full().gap_3().child(new Icon('icons/hard-drive.svg').size_5())
          .child(div().v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().truncate().child(profile.name))
            .child(div().text_sm().truncate().text_color(theme().colors.muted_foreground).child(description(profile,text))))
          .child(new Icon('icons/chevron-right.svg').size_4())))));
}
function feedback(view,form=false) {
  const {pending,text}=view;
  const content=div().v_flex().min_w_0();
  if(form)content.gap_3();else content.gap_2().px_3().py_1();
  return content
    .children(pending?[div().id('db-pending').h_flex().gap_2().items_center()
      .child(div().flex_1().text_sm().child(pending.request.unknown?'':text.db_running))
      .children(pending.request.unknown?[new Button('db-check-result').label(text.db_check_result).disabled(pending.request.running).on_click((_,cx)=>view.run(cx=>view.check(cx),cx))]:[])
      .children(['query_database','check_database'].includes(pending.request.command?.kind)?[new Button('db-stop').label(text.db_stop).on_click((_,cx)=>view.run(()=>pending.request.cancel(),cx))]:[])]:[]);
}
function results(view) {
  const {result,text}=view;
  const pages=Math.max(1,Math.ceil((result?.rows.length??0)/50));
  return div().id('db-results').v_flex().size_full().min_h_0().min_w_0()
    .child(div().flex_1().min_h_0().min_w_0().child(DataTable.new('database-results',{revision:String(view.epoch),columns:result?.columns??[],rows:(result?.rows??[]).map(row=>row.map(cell)),page:view.page,menu:view.rowMenu(),stripe:true,empty_stripes:true,header:!!result?.columns.length})))
    .children(pages>1||result?.truncated?[div().h_flex().p_2().gap_2().items_center().text_sm()
      .children(result.truncated?[div().text_color(theme().colors.muted_foreground).child(text.db_truncated)]:[])
      .child(div().flex_1())
      .children(pages>1?[new Pagination('db-pages').size('small').current_page(view.page+1).total_pages(pages).visible_pages(3).on_change((page,cx)=>{view.page=page-1;cx.notify();})]:[])]:[]);
}
function log(view) {
  const {text}=view;
  return div().id('database-history').relative().size_full().min_h_0().track_focus(view.logFocus).on_key_down((event,cx)=>view.logKey(event,cx))
    .child(DataTable.new('database-history-grid',{revision:String(view.epoch),columns:[],rows:[],stripe:true,empty_stripes:true,header:false}))
    .child(div().id('database-history-rows').absolute().inset_0().v_flex().overflow_y_scrollbar()
    .children([...view.logs].reverse().map((entry,index)=>{
      const result=entry.result?.rows??entry.request?.output?.data?.data,error=entry.result?.error??entry.request?.error;
      const status=entry.request?.unknown?text.db_unknown:error?text.db_failed:result?format(text.db_rows,{count:result.columns.length?result.rows.length:result.affected_rows}):text.db_running;
      const id=`db-log:${JSON.stringify({profile:view.opened,id:entry.id})}`;
      const items=[{id:`log:${JSON.stringify({profile:view.opened,id:entry.id,edit:false})}`,label:text.db_copy_query,enabled:true},
        {id:`log:${JSON.stringify({profile:view.opened,id:entry.id,edit:true})}`,label:text.db_open_sql,enabled:!view.locked()}];
      return div().id(`db-history-row-${entry.id}`).h_8().flex_none().child(NativeContextMenu.new(`db-history-${entry.id}`,{items}).child(Tooltip.new(`db-log-tooltip-${entry.id}`,{text:`${new Date(entry.timestamp).toLocaleTimeString()}\n${entry.sql}${error?.message?`\n${error.message}`:''}`})
        .child(SelectableRow.new(id,{variant:'list',selected:view.selectedLog===entry.id,stripe:index%2===1})
          .child(div().h_flex().w_full().min_w_0().gap_3()
            .child(div().id(`db-log-sql-${index}`).flex_1().min_w_0().truncate().text_color(theme().colors.foreground).child(entry.sql))
            .child(div().id(`db-log-status-${index}`).flex_none().child(status))
            .child(div().id(`db-log-elapsed-${index}`).flex_none().w_16().text_right().child(entry.elapsed==null?'':`${entry.elapsed.toFixed(2)}s`))))));
    })));
}
function sqlEditor(view) {
  const {text,pending}=view,unknown=pending?.request.unknown;
  const button=IconButton.new(pending?unknown?'db-inspect':'db-stop':'db-run',{
    icon:pending?unknown?'rotate-cw':'square':'play',label:text[pending?unknown?'db_inspect':'db_stop':'db_run'],
    variant:'primary',circular:true,size:'medium',disabled:!pending&&(view.locked()||!readText(view.sql).trim()),
  });
  return div().v_flex().size_full().min_h_0().px_3().pt_2().pb_4().justify_end()
    .child(InputSurface.new('db-sql-surface')
      .child(div().id('db-editor-scroll').min_h_0().overflow_hidden().child(TextField.new(view.sql,{disabled:view.locked(),appearance:false,bordered:false})))
      .child(div().h_flex().w_full().min_w_0().gap_2()
        .child(div().id('db-execution-status').pl_3().flex_1().min_w_0().truncate().text_sm().text_color(theme().colors.muted_foreground)
          .child(pending?unknown?'':text.db_running:text.db_ready))
        .child(div().id('db-execution-target').max_w('40%').min_w_0().truncate().text_sm().text_color(theme().colors.muted_foreground).child(view.catalog.database??view.profile().name))
        .child(button)));
}
function editorPanel(view) {
  const {text}=view;
  const tabs=new TabBar('db-bottom-tabs').variant('tab').rounded_none().px_3().menu(false).selected_index(view.tab)
    .children([new Tab().label(text.db_history),new Tab().label(text.db_sql_tab)])
    .on_change((index,cx)=>{view.tab=index;cx.notify();});
  return div().v_flex().size_full().min_h_0().min_w_0().child(tabs)
    .child(div().flex_1().min_h_0().child(view.tab===0?log(view):sqlEditor(view)));
}
function content(view) {
  if(!view.profile())return div().id('db-landing').v_flex().size_full().min_h_0().children(view.pending?[feedback(view)]:[]).child(landing(view));
  const panels=new Resizable('db-editor-results').axis('vertical')
    .child(new ResizablePanel().size_range(100,10000).child(results(view)))
    .child(new ResizablePanel().size(200).size_range(120,480).flex_none().child(editorPanel(view)));
  return div().id('db-page').v_flex().size_full().min_h_0().min_w_0().child(div().flex_1().min_h_0().overflow_hidden().child(panels));
}
function navigation(view) {
  const {text,catalog}=view;
  if(!view.profile())return div();
  return div().id('db-catalog').v_flex().size_full().min_h_0().min_w_0()
    .child(PanelHeader.new('database-catalog-header',{size:'row',draggable:false,bordered:true}).child(div().h_flex().w_full().items_center().justify_end()
      .child(IconButton.new('db-refresh-catalog',{icon:'rotate-cw',label:text.refresh,disabled:!!catalog.loading.size}))))
    .child(div().flex_1().min_h_0().p_1().child(ResourceTree.new('database-tree',{variant:'branch',items:catalog.items(text,(table,database)=>view.tableMenu(table,database)),current:catalog.current,selected:[]})));
}

export function render(view) {
  const {text}=view,profile=view.profile(),opened=!!view.editor||!!view.confirmation;
  const form=view.editor?{form:true,title:text[view.editor.original.revision?'db_edit':'db_new'],width:560}:{};
  const editorBody=view.editor?div().v_flex().gap_4().child(editor(view))
    .children(view.pending?[feedback(view,true)]:[]):null;
  const connecting=!opened&&(!!view.pending&&!view.pending.request.unknown&&view.pending.request.command?.kind==='check_database'||profile&&view.catalog.loading.size&&!view.catalog.names.length);
  return div().size_full().min_h_0()
    .child(Header.new('database-header',{content:JSON.stringify({title:text.database,
      tabs:profile?{id:'db-tabs',selected:profile.id,max_width:180,close_label:text.close,items:[{id:profile.id,label:profile.name,icon:'hard-drive',closable:true,close_disabled:!!view.pending}]}:undefined,
      actions:[{id:'db-connections-menu',label:text.db_connections,icon:'icons/hard-drive.svg',disabled:view.locked()}]})}))
    .child(Workspace.new('database-workspace',{has_details:!!profile,details_label:text.connection_assistant,min_navigation_width:200,min_details_width:320,navigation:profile?'resource':'none'}).children([
      div().relative().size_full().min_h_0().child(content(view)).children(connecting?[Loading.new('db-connecting')]:[]),
      profile?div().id('db-assistant-panel').size_full().min_h_0().child(Conversation.new(`database-assistant-${view.opened}`,{assistant:'database',resource:{kind:'database',id:view.opened},heading:text.connection_assistant})):div(),
      navigation(view)
    ])).child(picker(view)).child(Modal.new(`db-dialog-${view.dialog}`,{open:opened,...form}).children(view.editor?[editorBody,editorFooter(view)]:view.confirmation?[
    div().id('db-confirm').v_flex().w(Math.min(520,window.viewport_size().width-80)).gap_3()
      .child(div().text_lg().font_semibold().child(text[view.confirmation.label])).child(div().whitespace_normal().text_sm().child(view.confirmation.text))
      .child(div().h_flex().justify_end().gap_2().child(new Button('db-confirm-cancel').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
        .child(new Button('db-confirm-submit').danger().label(text[view.confirmation.label]).disabled(view.locked()).on_click((_,cx)=>view.confirm(cx))))
  ]:[]));
}
