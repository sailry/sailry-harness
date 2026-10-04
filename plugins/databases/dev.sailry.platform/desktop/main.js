import {View} from 'gpui-kit';
import {context,nextAssistantTools,header_action} from 'sailry';
import {nextChange,readProjectCatalog} from 'sailry/sdk';
import {listDatabases,listSsh,newDatabaseId,prepareDatabase} from 'sailry/connections';
import {createText,readText,setText,releaseText,focusText,nextTextEvent} from 'sailry/forms';
import {createSecret,releaseSecret,describeSecret} from 'sailry/credentials';
import {nextTableEvent,nextTreeEvent,nextContextMenuEvent,nextNavigationTabEvent,nextPickerEvent,nextControlEvent,modal_closed,writeClipboard,toast} from 'sailry/ui';
import {Request,failure} from './requests.js';
import {Catalog} from './catalog.js';
import {draft,prepare,switchEngine} from './editor.js';
import * as sql from './sql.js';
import * as history from './history.js';
import {messages,format} from './locales.js';
import {render} from './view.js';

export const command=(kind,data)=>({kind,data});
export const cell=value=>value.kind === 'null' ? 'NULL' : value.kind === 'blob' ? value.value.map(byte=>byte.toString(16).padStart(2,'0')).join('') : String(value.value);
export const rowText=rows=>rows.map(row=>row.map(cell).map(value=>/[\t\n\r"]/.test(value) ? `"${value.replace(/"/g,'""')}"` : value).join('\t')).join('\n');

export default class Databases extends View {
  init(_props,cx) {
    this.text=messages(JSON.parse(context()).locale); this.profiles=[]; this.ssh=[]; this.projects=[]; this.connected=true;
    this.opened=null; this.catalog=null; this.result=null; this.origin=null; this.epoch=0; this.page=0; this.logs=[]; this.selectedLog=null; this.logFocus=cx.focus_handle(); this.tab=0;
    this.pending=null; this.error=null; this.status=null; this.editor=null; this.confirmation=null; this.picker=false; this.pickerProfile=null; this.pickerRevision=0; this.dialog=0;
    this.sql=createText('',{multiline:true,rows:[2,6],placeholder:this.text.db_sql});
    this.histories=new Map();
    this.refreshing=null;this.refreshAgain=false;this.refreshProjects=false;this.refreshCatalog=false;
    this.run(cx=>this.refresh(cx,true),cx);
    let entry=null;
    this.watch(nextChange,(change,cx)=>{
      const entered=entry!==null&&entry!==change.entry;entry=change.entry;
      this.connected=change.connected;
      if(change.connected)this.run(cx=>this.refresh(cx,entered,entered),cx);
    },cx,true);
    this.watch(nextTableEvent,(event,cx)=>this.run(cx=>this.rowAction(event,cx),cx),cx);
    this.watch(nextTreeEvent,(event,cx)=>this.run(cx=>this.tree(event,cx),cx),cx);
    this.watch(nextContextMenuEvent,(event,cx)=>this.run(cx=>this.menu(event.id,cx),cx),cx);
    this.watch(header_action,(id,cx)=>{if(id==='db-connections-menu')this.connections(cx);},cx);
    this.watch(nextNavigationTabEvent,(event,cx)=>this.navigation(event,cx),cx);
    this.watch(nextPickerEvent,(event,cx)=>this.pick(event,cx),cx);
    this.watch(nextControlEvent,(event,cx)=>this.control(event,cx),cx);
    this.watch(nextTextEvent,(event,cx)=>{if(event.id===this.sql&&event.kind==='enter')cx.notify();},cx);
    this.watch(modal_closed,(id,cx)=>{if(id===`db-dialog-${this.dialog}`)this.close(cx);},cx);
    this.watch(nextAssistantTools,(value,cx)=>this.assistant(value.conversations,cx),cx,true);
  }
  watch(read,handle,cx,cursor=false) { cx.spawn(async cx=>{let seen='';try{while(true){const value=await read(...(cursor?[seen]:[]));if(cursor)seen=value.cursor;handle(value,cx);cx.notify();}}catch(_){}}); }
  run(action,cx) { cx.spawn(async cx=>{try{await action(cx);}catch(error){this.report({message:error.message});}finally{cx.notify();}}); }
  report(error) { this.error=error;const label=this.text[failure(error)];toast({id:'database-error',kind:'error',message:error.message&&error.message!==label?`${label}\n${error.message}`:label}); }
  notice(key) { toast({id:'database-success',kind:'info',message:this.text[key]}); }
  refresh(cx,projects=false,catalog=false) {
    this.refreshProjects ||= projects;this.refreshCatalog ||= catalog;
    if(this.refreshing){this.refreshAgain=true;return this.refreshing;}
    this.refreshing=this.reload(cx).finally(()=>{this.refreshing=null;});
    return this.refreshing;
  }
  async reload(cx) {
    do {
      this.refreshAgain=false;
      let projects=this.refreshProjects,catalog=this.refreshCatalog;
      this.refreshProjects=false;this.refreshCatalog=false;
      try {
        const [profiles,ssh]=await Promise.all([listDatabases(),listSsh()]);this.profiles=profiles;this.ssh=ssh;
        if(this.opened&&!this.profile()){this.opened=null;this.catalog=null;this.result=null;this.origin=null;this.epoch++;}
        if(projects){this.projects=(await readProjectCatalog()).projects;projects=false;}
        const current=this.catalog,profile=this.profile();
        if(catalog&&current&&profile&&!this.pending&&!current.loading.size){
          current.profile=profile;await current.load(null,cx);
          for(const database of current.expanded)await current.load(database,cx);
        }
      }catch(error){this.refreshProjects ||= projects;this.refreshCatalog ||= catalog;throw error;}
    }while(this.refreshAgain);
  }
  profile() { return this.profiles.find(profile=>profile.id===this.opened); }
  navigation(event,cx) {
    if(event.bar!=='db-tabs'||event.kind!=='close'||event.id!==this.opened||!this.profile()||this.pending)return;
    this.opened=null;this.catalog=null;this.result=null;this.origin=null;this.status=null;this.epoch++;this.page=0;cx.notify();
  }
  control(event,cx) {
    if(event.id==='db-new'){if(!this.locked())this.edit(null,cx);return;}
    if(event.id?.startsWith('db-open-')){const profile=this.profiles.find(profile=>event.id===`db-open-${profile.id}`);if(profile&&!this.locked())this.run(cx=>this.open(profile,cx),cx);return;}
    if(event.id==='db-refresh-catalog'){const catalog=this.catalog;if(catalog&&this.profile()&&!catalog.loading.size)this.run(cx=>catalog.load(null,cx,true),cx);return;}
    if(event.id==='db-run'){this.runSql(cx);return;}
    if(event.id==='db-stop'){const pending=this.pending;if(pending&&!pending.request.unknown)this.run(()=>pending.request.cancel(),cx);return;}
    if(event.id==='db-inspect'){if(this.pending?.request.unknown)this.run(cx=>this.check(cx),cx);return;}
    if(event.id?.startsWith('db-log:')){const target=JSON.parse(event.id.slice(7));if(target.profile!==this.opened)return;this.selectLog(target.id,cx);if(event.click_count===2)this.logAction(target.id,true,cx);return;}
    const editor=this.editor;if(!editor||this.locked())return;
    if(event.id===`db-engine-${this.dialog}`&&['sqlite','mysql','postgres'].includes(event.value))this.changeEngine(event.value,cx);
    if(event.id===`db-transport-${this.dialog}`&&['tcp','socket','ssh'].includes(event.value))editor.method=event.value;
    if(event.id===`db-tls-${this.dialog}`&&['disable','prefer','require'].includes(event.value))editor.tls=event.value;
    if(event.id===`db-ssh-${this.dialog}`&&this.ssh.some(profile=>profile.id===event.value))editor.ssh=event.value;
    cx.notify();
  }
  connections(cx) {if(this.locked())return;this.picker=true;this.pickerProfile=null;this.pickerRevision++;cx.notify();}
  pick(event,cx) {
    if(!this.picker||event.picker!==`db-picker-${this.pickerRevision}`)return;
    if(event.kind==='close'){this.picker=false;cx.notify();return;}if(this.locked()||event.kind!=='select')return;
    const choice=event.value;
    if(choice?.kind==='profile'){if(!this.profiles.some(profile=>profile.id===choice.id))return;this.pickerProfile=choice.id;this.pickerRevision++;cx.notify();return;}
    if(choice?.kind==='back'){this.pickerProfile=null;this.pickerRevision++;cx.notify();return;}
    if(choice?.kind==='add'){this.picker=false;this.edit(null,cx);return;}
    const profile=this.profiles.find(profile=>profile.id===this.pickerProfile);if(!profile||choice?.kind!=='action')return;
    this.picker=false;
    if(choice.action==='open')this.run(cx=>this.open(profile,cx),cx);
    if(choice.action==='edit')this.edit(profile,cx);
    if(choice.action==='remove')this.remove(profile,cx);
    if(choice.action==='check')this.run(cx=>this.execute(command('check_database',{profile:profile.id,expected_revision:profile.revision}),cx),cx);
    cx.notify();
  }
  valid(origin) { return !!origin && this.opened===origin.profile.id && JSON.stringify(this.profile())===JSON.stringify(origin.profile); }
  locked() {return !this.connected || !!this.pending;}
  async open(profile,cx) {
    if(this.locked())return;
    if(this.opened!==profile.id) {
      this.opened=profile.id;this.logs=[];this.selectedLog=null;this.result=null;this.origin=null;this.status=null;this.error=null;this.page=0;this.epoch++;
      this.catalog=new Catalog(profile,error=>this.report(error));
    } else this.catalog.profile=profile;
    this.picker=false;await this.catalog.load(null,cx);
  }
  query(sqlText,database,readOnly,profile=this.profile()) {
    return command('query_database',{profile:profile.id,expected_revision:profile.revision,database:database??null,sql:sqlText,read_only:readOnly,row_limit:1000,timeout_ms:30000});
  }
  async execute(value,cx,origin=null,after=null,id=null,present=true) {
    if(this.pending||!this.connected)return null;
    const request=new Request(value,id); this.pending={request,origin,after,present};this.error=null;this.status=null;
    if(value?.kind==='query_database') {this.logs.push({id:request.id,sql:value.data.sql,database:value.data.database,timestamp:Date.now(),request});if(this.logs.length>100)this.logs.shift();}
    await request.start(cx);if(request.unknown)this.report(request.uncertainty??{code:'outcome_unknown'});return await this.finish(cx);
  }
  async finish(cx) {
    const pending=this.pending;if(!pending||!pending.request.done)return null;
    const {request,origin,after}=pending;this.pending=null;request.release();
    const entry=this.logs.find(entry=>entry.id===request.id);if(entry&&!entry.elapsed)entry.elapsed=(Date.now()-entry.timestamp)/1000;
    if(request.error){this.report(request.error);return null;}
    const output=request.output;
    if(pending.present!==false&&output.kind==='database_outcome'&&output.data.kind==='query') {
      this.result=output.data.data;this.origin=origin;this.epoch++;this.page=0;
    } else if(output.kind==='database_outcome'&&output.data.kind==='connected')this.notice('db_connected');
    if(after)await after(output,cx);cx.notify();return output;
  }
  async check(cx) {if(this.pending){const request=this.pending.request;await request.check(cx);if(request.unknown)this.report(request.uncertainty??{code:'outcome_unknown'});await this.finish(cx);}}
  runSql(cx) {
    const text=readText(this.sql).trim(),profile=this.profile();if(!text||!profile||this.locked())return;
    const origin={profile:JSON.parse(JSON.stringify(profile)),database:this.catalog.database,sql:text,table:null,keys:[]};
    this.run(cx=>this.execute(this.query(text,origin.database,profile.read_only),cx,origin),cx);
  }
  async readRows(origin,cx) {
    if(!this.valid(origin)||this.locked())return;
    await this.execute(this.query(origin.sql,origin.database,true,origin.profile),cx,origin,async()=>{
      if(!this.valid(origin))return;
      const keys=sql.primaryKeys(this.engine(origin.profile),origin.table);
      if(!keys)return;
      const request=new Request(this.query(keys,origin.database,true,origin.profile));await request.start(cx);
      if(request.output?.data?.kind==='query'&&this.origin===origin)origin.keys=request.output.data.data.rows.flatMap(row=>row[0]?.kind==='text'?[row[0].value]:[]);
      if(request.done)request.release();else {this.pending={request,origin:null,present:false,after:output=>{if(this.origin===origin)origin.keys=output.data.data.rows.flatMap(row=>row[0]?.kind==='text'?[row[0].value]:[]);}};}
    });
  }
  engine(profile=this.profile()) {return profile.connection.kind==='sqlite'?'sqlite':profile.connection.engine;}
  tableOrigin(target) {const profile=this.profile();return {profile:JSON.parse(JSON.stringify(profile)),...target,keys:[],sql:`SELECT * FROM ${sql.tableName(this.engine(profile),target.table)} LIMIT 1000`};}
  tableMenu(table,database) {
    const identity=JSON.stringify({epoch:this.epoch,profile:this.profile(),database,table});
    return ['open','structure','name','select',...(this.engine()==='postgres'?[]:['definition']),'clear','drop'].map(action=>({id:`table:${action}:${identity}`,label:this.text[{open:'db_table_open',structure:'db_table_structure',name:'db_table_copy_name',select:'db_table_copy_select',definition:'db_table_copy_definition',clear:'db_table_clear',drop:'db_table_drop'}[action]],enabled:['name','select'].includes(action)||!this.locked()&&(!['clear','drop'].includes(action)||!this.profile().read_only)}));
  }
  async tree(event,cx) {
    if(event.tree!=='database-tree'||!this.catalog)return;
    const target=this.catalog.target(event.id);if(!target)return;
    if(event.kind==='select')this.catalog.current=event.id;
    if(event.kind==='expand'){this.catalog.expanded.add(target.database);await this.catalog.load(target.database,cx);}
    if(event.kind==='open'&&event.id.startsWith('retry:'))await this.catalog.load(target.database,cx);
    if(event.kind==='collapse')this.catalog.expanded.delete(target.database);
    if(event.kind==='open'&&target.table){this.catalog.database=target.database;await this.readRows(this.tableOrigin(target),cx);}
    if(event.kind==='menu')await this.menu(event.action,cx);
  }
  async menu(id,cx) {
    if(id?.startsWith('log:')){const target=JSON.parse(id.slice(4));if(target.profile!==this.opened)return;return this.logAction(target.id,target.edit,cx);}
    if(!id?.startsWith('table:'))return;
    const end=id.indexOf(':',6),action=id.slice(6,end),target=JSON.parse(id.slice(end+1));
    if(target.epoch!==this.epoch||JSON.stringify(target.profile)!==JSON.stringify(this.profile()))return;
    const origin=this.tableOrigin(target),engine=this.engine(),name=sql.tableName(engine,target.table);
    if(action==='name')return writeClipboard(target.table.name);
    if(action==='select')return writeClipboard(origin.sql);
    if(this.locked())return;
    this.catalog.database=target.database;
    if(action==='open')return this.readRows(origin,cx);
    if(action==='structure')return this.execute(this.query(sql.structure(engine,target.table),target.database,true),cx,{...origin,table:null});
    if(action==='definition')return this.execute(this.query(sql.definition(engine,target.table),target.database,true),cx,null,output=>{const text=sql.definitionText(engine,output.data.data);if(text)writeClipboard(text);},null,false);
    if(this.profile().read_only)return;
    this.confirmation={label:`db_table_${action}`,text:format(this.text[`db_table_${action}_confirm`],{table:name}),origin,
      run:cx=>this.execute(this.query(`${action==='clear'?'DELETE FROM':'DROP TABLE'} ${name}`,target.database,false),cx,null,async()=>{if(action==='clear')await this.readRows(origin,cx);else await this.catalog.load(target.database,cx);})};this.dialog++;cx.notify();
  }
  rowMenu() {const writable=this.origin?.table&&!this.origin.profile.read_only&&!this.locked();return ['cell','row','query','insert','update','delete_sql','edit','delete'].map(action=>({id:action,label:this.text[{cell:'db_copy_cell',row:'db_copy_row',query:'db_copy_query',insert:'db_copy_insert',update:'db_copy_update',delete_sql:'db_copy_delete',edit:'db_edit_row_sql',delete:'db_delete_row'}[action]],enabled:action==='delete'?!!writable:['insert','update','delete_sql','edit'].includes(action)?!!this.origin?.table:true}));}
  async rowAction(event,cx) {
    if(event.table!=='database-results'||event.revision!==String(this.epoch)||!this.valid(this.origin)||!this.result)return;
    if(event.kind==='selection')return;
    const action=event.kind==='copy'?'row':event.action,indices=event.rows.length?event.rows:event.row==null?[]:[event.row],rows=indices.map(index=>this.result.rows[index]);
    if(!rows.length||rows.some(row=>!row))return;
    if(action==='cell')return writeClipboard(cell(this.result.rows[event.row][event.column]));
    if(action==='row')return writeClipboard(rowText(rows));
    if(action==='query')return writeClipboard(this.origin.sql);
    const origin=this.origin,kind=['delete','delete_sql'].includes(action)?'delete':action==='edit'?'update':action;
    if(!origin.table)return;
    const text=sql.statements(this.engine(origin.profile),origin.table,origin.keys,this.result.columns,rows,kind);if(!text)return;
    if(action==='edit') {if(rows.length!==1||this.locked())return;this.openSql(text,origin.database,cx);return;}
    if(action!=='delete')return writeClipboard(text);
    if(this.locked()||origin.profile.read_only)return;
    this.confirmation={label:'db_delete_row',text:format(this.text.db_delete_row_confirm,{table:sql.tableName(this.engine(),origin.table),count:rows.length,sql:text}),origin,
      run:cx=>this.execute(this.query(text,origin.database,false,origin.profile),cx,null,()=>this.readRows(origin,cx))};this.dialog++;cx.notify();
  }
  openSql(text,database,cx) {if(this.locked())return;setText(this.sql,text);this.catalog.database=database;this.tab=1;focusText(this.sql);cx.notify();}
  selectLog(id,cx) {if(!this.logs.some(entry=>entry.id===id))return;this.selectedLog=id;this.logFocus.focus();cx.notify();}
  logAction(id,edit,cx) {const entry=this.logs.find(entry=>entry.id===id);if(!entry)return;if(edit)this.openSql(entry.sql,entry.database,cx);else writeClipboard(entry.sql);}
  logKey(event,cx) {if(!this.logs.length||this.tab!==0)return;
    const index=this.logs.findIndex(entry=>entry.id===this.selectedLog);
    if(['up','down'].includes(event.key)){const next=index<0?this.logs.length-1:event.key==='up'?Math.min(index+1,this.logs.length-1):Math.max(index-1,0);this.selectedLog=this.logs[next].id;cx.stop_propagation();cx.notify();return;}
    const copy=event.key==='c'&&(event.modifiers.platform||event.modifiers.control);
    if(this.selectedLog&&(copy||event.key==='enter')){this.logAction(this.selectedLog,!copy,cx);cx.stop_propagation();}
  }
  edit(profile,cx) {
    if(this.locked())return;this.close(cx);const value=draft(profile,newDatabaseId());
    value.inputs=value.fields.map((text,index)=>createText(text,{label:this.text[['settings_name','db_path','db_host','db_port','db_name','db_username'][index]],placeholder:this.text[['form_name_hint','db_path_hint','form_host_hint','form_port_hint','form_database_hint','form_username_hint'][index]]}));
    value.secret=createSecret({label:this.text.db_password,placeholder:this.text.form_password_hint});this.editor=value;this.dialog++;this.picker=false;cx.notify();
  }
  changeEngine(engine,cx) {const editor=this.editor;editor.fields=editor.inputs.map(readText);switchEngine(editor,engine);setText(editor.inputs[3],editor.fields[3]);cx.notify();}
  save(testing,cx) {
    if(!this.editor||this.locked())return;const editor=this.editor;
    this.run(async cx=>{editor.fields=editor.inputs.map(readText);const profile=prepare(editor,this.ssh),id=prepareDatabase({profile,secret:describeSecret(editor.secret).filled?editor.secret:null,testing});
      await this.execute(null,cx,null,async()=>{await this.refresh(cx);if(!testing&&this.editor===editor){this.close(cx);this.notice('db_saved');}},id);},cx);
  }
  close(cx) {if(this.editor){this.editor.inputs.forEach(releaseText);releaseSecret(this.editor.secret);}this.editor=null;this.confirmation=null;this.picker=false;cx.notify();}
  remove(profile,cx) {if(this.locked())return;this.confirmation={label:'settings_delete',text:format(this.text.db_remove_confirm,{name:profile.name}),run:cx=>this.execute(command('remove_database',{profile:profile.id,expected_revision:profile.revision}),cx,null,()=>this.refresh(cx))};this.dialog++;this.picker=false;cx.notify();}
  confirm(cx) {const confirmation=this.confirmation;if(!confirmation||this.locked()||confirmation.origin&&!this.valid(confirmation.origin))return;this.confirmation=null;this.run(confirmation.run,cx);}
  assistant(conversations,cx) {
    for(const [id,snapshot] of Object.entries(conversations)) {
      const previous=this.histories.get(id);this.histories.set(id,snapshot);
      for(const call of history.updates(snapshot,previous)) {
        if(call.arguments.connection!==this.opened)continue;
        let entry=this.logs.find(entry=>entry.id===call.key);if(!entry){entry={id:call.key,sql:call.arguments.sql,database:call.arguments.database,timestamp:call.timestamp_ms};this.logs.push(entry);if(this.logs.length>100)this.logs.shift();}
        const result=history.result(call);entry.result=result;
        if(result.rows){this.result=result.rows;this.origin={profile:JSON.parse(JSON.stringify(this.profile())),database:entry.database,sql:entry.sql,table:null,keys:[]};this.epoch++;this.page=0;if(call.live)this.error=null;}
        if(result.error&&call.live)this.report(result.error);
      }
    }cx.notify();
  }
  render() {return render(this);}
}
