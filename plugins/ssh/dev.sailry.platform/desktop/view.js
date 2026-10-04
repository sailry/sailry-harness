import {div} from 'gpui-kit';
import {Button,VForm,Field,Progress,Icon} from 'gpui-component';
import {Conversation,Terminal,Header,theme} from 'sailry';
import {Workspace,PanelHeader,EmptyState,Loading,Picker,SelectableRow,IconButton,SelectField,ResourceTree,NativeContextMenu,Modal,ActionScope} from 'sailry/ui';
import {TextField} from 'sailry/forms';
import {format} from './locales.js';
import {editor,editorFooter} from './editor-view.js';
const description=profile=>`${profile.username}@${profile.host}:${profile.port}`;
function picker(view){const{text}=view,profile=view.profiles.find(profile=>profile.id===view.pickerProfile),disabled=view.locked();
  const actions=[['open','connection_action_open','arrow-right'],['edit','connection_action_edit','settings-2'],['check','connection_action_check','check'],['transfer','ssh_transfer','replace'],['run','ssh_run','play'],['remove','settings_delete','delete']];
  const items=profile?[...actions.map(([action,label,icon])=>({id:`connection-action-${action}`,label:text[label],icon,disabled,value:{kind:'action',action}})),
    {id:'connection-back',label:text.reference_back,icon:'chevron-left',value:{kind:'back'}}]:[
    ...view.profiles.map((profile,index)=>({id:`connection-choice-${index}`,label:profile.name,detail:description(profile),icon:'network',submenu:true,disabled,value:{kind:'profile',id:profile.id}})),
    {id:'connection-add',label:text.ssh_new,icon:'plus',disabled,value:{kind:'add'}}];
  return Picker.new(`ssh-picker-${view.pickerRevision}`,{mode:'command',open:view.picker,title:text.connection_search,empty:text.connection_search_empty,items});}
function landing(view){const{text}=view;return EmptyState.new('ssh_workspace_empty',{icon:'square-terminal',label:text.ssh_workspace_empty})
  .child(div().w_full().max_w_96().child(IconButton.new('ssh-new',{icon:'plus',label:text.ssh_new,show_label:true,full_width:true,variant:'primary',size:'medium',disabled:view.locked()})))
  .child(div().id('ssh-saved-connections').v_flex().w_full().max_w_96().max_h(320).overflow_y_scroll().gap_2()
    .children(view.profiles.map(profile=>SelectableRow.new(`ssh-open-${profile.id}`,{variant:'card',label:profile.name,disabled:view.locked(),selected:false})
      .child(div().h_flex().w_full().gap_3().child(new Icon('icons/network.svg').size_5())
        .child(div().v_flex().flex_1().min_w_0().gap_0p5().child(div().text_sm().truncate().child(profile.name))
          .child(div().text_sm().truncate().text_color(theme().colors.muted_foreground).child(description(profile))))
        .child(new Icon('icons/chevron-right.svg').size_4())))));}
function feedback(view,form=false){const{text,outcome,pending}=view,content=div().v_flex().min_w_0();if(form)content.gap_3();else content.gap_2().px_3().py_2();return content
  .children(outcome?.kind==='completed'?[
    ...[outcome.stdout,outcome.stderr].flatMap((value,index)=>value?[new Field().label(text[index?'command_stderr':'command_stdout']).child(TextField.new(view.output[index],{readonly:true}))]:[]),
    ...(outcome.truncated?[div().text_sm().child(text.command_truncated)]:[])]:[])
  .children(pending?[div().id('ssh-pending').h_flex().gap_2().items_center().child(div().text_sm().flex_1().child(pending.request.unknown?'':text.ssh_running))
    .children(pending.request.unknown?[new Button('ssh-inspect').label(text.ssh_inspect).disabled(pending.request.running).on_click((_,cx)=>view.run(cx=>view.inspect(cx),cx))]:[])
    .child(new Button('ssh-stop').label(text.ssh_stop).on_click((_,cx)=>view.run(()=>pending.request.cancel(),cx)))]:[]);}
function files(view){const files=view.files(),{text}=view;if(!files)return div();const empty=!files.pending&&!files.error&&files.revision>0&&!files.entries.length;return div().id('ssh-files-panel').v_flex().size_full().min_h_0().min_w_0()
  .child(PanelHeader.new('ssh-files-header',{size:'row',padding:8,draggable:false,bordered:true}).child(div().h_flex().w_full().gap_1().items_center()
    .child(div().flex_1().min_w_0().child(TextField.new(files.input,{disabled:files.pending,size:'small',appearance:false,bordered:false})))
    .child(IconButton.new('ssh-files-parent',{icon:'arrow-up',label:text.ssh_files_parent,disabled:files.pending||files.path==='/'}))))
  .child(div().v_flex().flex_1().min_h_0().min_w_0().p_2().child(NativeContextMenu.new('ssh-files-root',{items:view.fileMenu('')}).child(empty
    ? EmptyState.new('ssh-files-empty',{variant:'list',fill_height:true,icon:'folder',label:text.files_directory_empty})
    : ResourceTree.new('ssh-files',{external_files:!view.locked(),items:files.items(path=>view.fileMenu(path)),selected:files.selected,current:files.current,menu:view.fileMenu('')}))))
  .children(files.next?[new Button('ssh-files-more').ghost().label(text.files_load_more).disabled(files.pending).on_click((_,cx)=>view.run(cx=>files.load(files.path,cx,true),cx))]:[])
  .child(div().h_8().h_flex().px_2().gap_2().text_xs().text_color(theme().colors.muted_foreground).child(format(text.ssh_files_count,{count:files.entries.length})))
  .children(view.jobs.filter(job=>job.profile===files.profile.id).map(job=>transferJob(view,job)));}
function transferJob(view,job){const{text}=view,progress=job.progress?job.progress.size?job.progress.copied/job.progress.size*100:100:null;return div().id(`ssh-transfer-${job.id}`).v_flex().p_2().gap_1().text_xs()
  .child(div().truncate().child(job.source))
  .children(progress!==null?[new Progress(`ssh-progress-${job.id}`).value(progress).accessibility_label(text.ssh_transfer)]:[])
  .children(job.can_replace?[div().child(text.ssh_files_exists)]:[])
  .child(div().h_flex().gap_1()
    .children(job.can_replace?[new Button(`ssh-replace-${job.id}`).size('small').label(text.ssh_files_replace).on_click((_,cx)=>view.transferAction(job,'replace',cx))]:[])
    .children(job.can_check?[new Button(`ssh-transfer-check-${job.id}`).size('small').label(text.ssh_inspect).on_click((_,cx)=>view.transferAction(job,'check',cx))]:[])
    .children(job.can_start?[new Button(`ssh-transfer-start-${job.id}`).size('small').label(text.retry).on_click((_,cx)=>view.transferAction(job,'start',cx))]:[])
    .child(new Button(`ssh-transfer-close-${job.id}`).ghost().size('small').label(job.can_cancel?text.settings_cancel:text.close).on_click((_,cx)=>view.transferAction(job,job.can_cancel?'cancel':'dismiss',cx))));}
function transfer(view){const{text,transfer:value}=view,items=view.worktrees.filter(tree=>tree.project).map(tree=>({id:tree.id,label:`${view.projects.find(project=>project.id===tree.project)?.name??''} · ${tree.path}`}));
  const trees=SelectField.new(`ssh-worktree-${view.dialog}`,{label:text.ssh_worktree,placeholder:text.ssh_no_project,selected:value.worktree,
    disabled:view.locked()||!items.length,items});
  return div().id('ssh-transfer-editor').v_flex().gap_4().child(new VForm()
    .child(new Field().label(text.ssh_worktree).child(trees))
    .child(new Field().label(text.ssh_project_path).child(TextField.new(value.inputs[0],{disabled:view.locked()})))
    .child(new Field().label(text.ssh_remote_path).child(TextField.new(value.inputs[1],{disabled:view.locked()}))))
    .children(view.pending?[feedback(view,true)]:[]);}
function transferFooter(view){const{text,transfer:value}=view;return div().h_flex().justify_end().gap_2().flex_wrap()
      .child(new Button('ssh-transfer-cancel').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
      .child(new Button('ssh-download').label(text.ssh_download).disabled(view.locked()||!value.worktree).on_click((_,cx)=>view.transferFile('download',cx)))
      .child(new Button('ssh-upload').primary().label(text.ssh_upload).disabled(view.locked()||!value.worktree).on_click((_,cx)=>view.transferFile('upload',cx)));}
function titled(title,body,footer){return {props:{form:true,title,width:560},children:[body,footer]};}
function dialog(view){const{text}=view;if(view.outcome?.kind==='host_key_required')return titled(text.ssh_trust_title,
  div().v_flex().gap_4().child(div().id('ssh-host-key').v_flex().gap_2()
    .child(div().text_color(theme().colors[view.outcome.changed?'destructive':'foreground']).child(text[view.outcome.changed?'ssh_key_changed':'ssh_key_unknown']))
    .child(div().whitespace_normal().text_sm().text_color(theme().colors.muted_foreground).child(view.outcome.key.fingerprint)))
    .children(view.pending?[feedback(view,true)]:[]),
  div().h_flex().justify_end().gap_2().child(new Button('ssh-trust-cancel').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
    .child(new Button('ssh-trust').primary().label(text.ssh_trust).disabled(view.locked()).on_click((_,cx)=>view.trust(cx))));
  if(view.confirmation)return {props:{},children:[div().id('ssh-confirm').v_flex().w(480).gap_3().child(div().text_lg().font_semibold().child(text[view.confirmation.label]))
    .child(div().whitespace_normal().text_sm().child(view.confirmation.text)).child(div().h_flex().justify_end().gap_2()
      .child(new Button('ssh-confirm-cancel').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
      .child(new Button('ssh-confirm-submit').danger().label(text[view.confirmation.label]).disabled(view.locked()).on_click((_,cx)=>view.confirm(cx))))]};
  if(view.editor)return titled(text[view.editor.original.revision?'ssh_edit':'ssh_new'],
    div().v_flex().gap_4().child(editor(view)).children(view.pending?[feedback(view,true)]:[]),editorFooter(view));
  if(view.fileEdit)return {props:{},children:[div().id('ssh-file-editor').v_flex().w(360).gap_3().child(TextField.new(view.fileEdit.input,{disabled:view.locked()}))
    .child(div().h_flex().justify_end().gap_2().child(new Button('ssh-file-cancel').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
      .child(new Button('ssh-file-submit').primary().label(text.settings_save).disabled(view.locked()).on_click((_,cx)=>view.submitFile(cx))))]};
  if(view.transfer)return titled(text.ssh_transfer,transfer(view),transferFooter(view));
  if(view.commandOpen)return titled(text.ssh_run,div().id('ssh-command').v_flex().gap_4()
    .child(TextField.new(view.command,{disabled:view.locked()})).children(view.pending||view.outcome?.kind==='completed'?[feedback(view,true)]:[]),
    div().h_flex().justify_end().gap_2().child(new Button('ssh-command-close').label(text.settings_cancel).on_click((_,cx)=>view.close(cx)))
      .child(new Button('ssh-run').primary().label(text.ssh_run).disabled(view.locked()).on_click((_,cx)=>view.runCommand(cx))));
  return {props:{},children:[]};}
export function render(view){const{text}=view,active=view.terminals.find(info=>info.id===view.terminal),profile=view.activeProfile(),opened=!!view.editor||!!view.confirmation||!!view.fileEdit||!!view.transfer||view.commandOpen||view.outcome?.kind==='host_key_required';
  const modal=opened?dialog(view):{props:{},children:[]};
  const connecting=!opened&&!!view.pending&&!view.pending.request.unknown&&['check_ssh','open_ssh_terminal'].includes(view.pending.request.command?.kind);
  const body=div().id(active?'ssh-page':'ssh-landing').relative().v_flex().size_full().min_h_0().min_w_0()
    .children(!opened&&!connecting&&view.pending?[feedback(view)]:[])
    .child(active?div().flex_1().min_h_0().child(Terminal.new(`ssh-terminal-${active.id}`,{terminal:active.id})):landing(view))
    .children(connecting?[Loading.new('ssh-connecting')]:[]);
  return ActionScope.new('ssh-actions',{close:'ssh-close-current'})
    .child(Header.new('ssh-header',{content:JSON.stringify({title:text.ssh,tabs:view.terminals.length?{id:'ssh-tabs',selected:view.terminal,close_label:text.close,max_width:180,items:view.terminals.map(info=>({id:info.id,label:view.profiles.find(profile=>profile.id===info.ssh)?.name??text.ssh,closable:true,icon:'terminal'}))}:undefined,
      actions:[{id:'ssh-connections-menu',label:text.ssh_connections,icon:'icons/network.svg',disabled:view.locked()}]})}))
    .child(Workspace.new('ssh-workspace',{has_details:!!profile,details_label:text.ssh_assistant,min_navigation_width:200,min_details_width:320,navigation:profile?'resource':'none'}).children([
      body,
      profile?div().id('ssh-assistant-panel').size_full().min_h_0().child(Conversation.new(`ssh-assistant-${profile.id}`,{assistant:'ssh',resource:{kind:'ssh',id:profile.id},heading:text.ssh_assistant})):div(),
      profile?files(view):div()
    ])).child(picker(view)).child(Modal.new(`ssh-dialog-${view.dialog}`,{open:opened,...modal.props}).children(modal.children));}
