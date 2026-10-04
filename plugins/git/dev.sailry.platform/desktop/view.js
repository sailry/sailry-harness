// Accepted Git layout composed with Kit controls and retained native diff leaves.
import {div} from 'gpui-kit';
import {Button, Icon, StatusBar, Pagination} from 'gpui-component';
import {Header,theme} from 'sailry';
import {TextField} from 'sailry/forms';
import {PanelHeader, NavigationTabs, IconButton, SelectableRow, ResourceTree, NativeContextMenu, Menu, DiffSurface, SourceSurface, Modal, Picker, SegmentedTabs, EmptyState, Collapse, CollapseSlot} from 'sailry/ui';
import {rows, checkbox, selectedId} from './changes.js';
export const defaultOpen = true;
const fileName = path => path.split('/').at(-1);
const parent = path => path.includes('/') ? path.slice(0,path.lastIndexOf('/')) : '';
const click = (view,value) => (_,cx) => view.run(() => view.actions.invoke(value),cx);

export function empty(view,changes = false) {
  const repo = view.repo, colors = theme().colors;
  let title = !repo.status ? (repo.error ? null : 'git_loading') : repo.status.kind === 'directory' ? 'git_directory_title'
    : repo.status.kind === 'unborn' ? 'git_unborn_title' : changes ? 'git_no_changes' : 'git_diff_empty';
  if (changes && repo.status && repo.status.kind !== 'directory') return div().flex_1().min_h_0().px_4().py_6().text_sm().text_color(colors.muted_foreground).child(view.text.git_no_changes);
  if (!title) return div().flex_1().min_h_0();
  const empty = EmptyState.new(changes ? `${title}-sidebar` : title,{...(changes ? {variant:'list',fill_height:true} : {}),icon:'network',label:view.text[title]});
  return !changes && repo.status?.kind === 'directory' ? empty.child(div().w_full().max_w_96()
    .child(IconButton.new('git-initialize',{icon:'plus',label:view.text.git_initialize,show_label:true,full_width:true,variant:'primary',size:'medium',disabled:!repo.canInitialize()}))) : empty;
}

export function header(view) {
  const tabs = {id:'git-tabs',
    items:view.repo.tabs.map(id => ({id,label:label(view,view.repo.documents.get(id)),closable:true,close_label:view.text.close})),
    selected:view.repo.selected,max_width:180,close_label:view.text.close
  };
  if (view.mode === 'main') return Header.new('git-header',{content:JSON.stringify({title:view.text.git,tabs})});
  return PanelHeader.new('git-tabs-header',{padding:8}).child(div().w_full().min_w_0().child(NavigationTabs.new(tabs.id,tabs)));
}
function label(view,document) {
  const request = document?.request;
  return request?.kind === 'file' ? fileName(request.path) : request?.kind === 'changes' ? view.text.git_all_changes
    : request?.kind === 'output' ? view.text.git_output : request?.kind === 'stash' ? view.text.git_stash_menu : request?.commit?.slice(0,8) ?? '';
}
function stats(additions,deletions,colors) {
  return [...(additions ? [div().text_xs().flex_shrink(0).text_color(colors.success).child(`+${additions}`)] : []),
    ...(deletions ? [div().text_xs().flex_shrink(0).text_color(colors.destructive).child(`−${deletions}`)] : [])];
}
function surface(view,file,id,maxRows) {
  if (file.binary) return div().id(`${id}-binary`).px_3().py_2().text_sm()
    .text_color(theme().colors.muted_foreground).child(view.text.git_diff_binary);
  const element = DiffSurface.new(id,{path:file.path,text:file.text,...(maxRows ? {max_rows:maxRows} : {})});
  return maxRows ? element : div().w_full().min_h_0().min_w_0().flex_1().child(element);
}
function combined(view,document) {
  const colors = theme().colors, working = document.request.kind === 'changes';
  return div().id('git-combined-diff').v_flex().size_full().p_2().gap_2().overflow_y_scroll()
    .children(!document.files.length ? [empty(view,true)] : document.files.map(file => {
      const id = `${document.id}/${file.path}`;
      const undoable = working && view.repo.status?.entries.some(entry => entry.path === file.path && !entry.conflicted);
      return Collapse.new(`diff-file-${id}`,{label:file.path,default_open:defaultOpen})
        .child(CollapseSlot.new(`git-diff-heading-${id}`,{variant:'header'})
          .child(div().h_flex().w_full().min_w_0().gap_2().text_sm()
              .child(div().h_flex().flex_1().min_w_0().gap_2().child(div().min_w_0().truncate().child(fileName(file.path)))
                .children(parent(file.path) ? [div().min_w_0().truncate().text_color(colors.muted_foreground).child(parent(file.path))] : []))
              .children(stats(file.additions,file.deletions,colors))))
        .children(undoable ? [CollapseSlot.new(`git-diff-actions-${id}`,{variant:'actions'})
          .child(IconButton.new(`git-diff-undo-${file.path}`,{icon:'undo',label:view.text.turn_changes_undo,disabled:!view.repo.writable()}))] : [])
        .child(CollapseSlot.new(`git-diff-body-${file.path}`,{variant:'content'})
          .child(div().v_flex()
            .children(file.binary || file.text ? [surface(view,file,`git-diff-${id}`,24)] : [])
            .children(file.truncated && !file.binary ? [div().px_3().py_2().text_sm().text_color(colors.muted_foreground).child(view.text.git_diff_partial)] : [])));
    }));
}
export function content(view) {
  const document = view.repo.documents.get(view.repo.selected), colors = theme().colors;
  const request = document?.request, files = document?.files ?? [];
  const additions = files.reduce((sum,file) => sum + file.additions,0), deletions = files.reduce((sum,file) => sum + file.deletions,0);
  const path = !document ? '' : request.kind === 'file' ? request.path : label(view,document);
  const menu = request?.kind === 'file' ? [{id:'open',label:view.text.workspace_open_file,enabled:true,value:{kind:'system',path:request.path}},{id:'copy',label:view.text.workspace_copy_path,enabled:true,value:{kind:'copy',text:request.path}}] : [];
  return div().id('git-diff').v_flex().size_full().min_h_0().min_w_0().track_focus(view.contentFocus)
    .child(div().id('document-editor').v_flex().flex_1().min_h_0().min_w_0()
      .child(!document ? empty(view) : request.kind === 'output' ? SourceSurface.new(`git-output-${document.id}`,{text:document.text ?? '',label:view.text.git_output})
        : request.kind === 'file' || request.kind === 'stash' ? (files[0] ? surface(view,files[0],`git-diff-${document.id}`) : empty(view)) : combined(view,document)))
    .children(document && request.kind !== 'output' ? [div().id('document-path').min_w_0().flex_shrink(0).child(new StatusBar().h_8().border_t_0().bg('#00000000')
      .child(NativeContextMenu.new('git-document-path',{items:menu.map(nativeItem)})
        .child(div().flex_1().min_w_0().truncate().child(path)))
      .child(div().h_flex().gap_2().children(stats(additions,deletions,colors))))] : []);
}
function nativeItem(item) {
  return item.separator ? {separator:true} : {id:item.id,label:item.label,enabled:item.enabled,value:item.value};
}
function descendants(row) { return row.entry ? [row.entry] : row.entries ?? row.children.flatMap(descendants); }
function decorated(view,row) {
  const entries = descendants(row), prefix = row.id.split('/')[0], scope = ['staged','unstaged'].includes(prefix) ? prefix : 'all';
  const expanded = view.expanded.get(row.id) !== false;
  view.nodes.set(row.id,{row,entries,scope});
  const menu = view.menus.files(entries,scope,row.id.startsWith('dir/')).map(nativeItem);
  const result = {id:row.id,label:row.label,expanded,menu,children:(row.children ?? []).map(row => decorated(view,row)),decoration:{selector:`git-${row.id.replace('/','-')}`}};
  if (row.entry) {
    const entry = row.entry, selected = checkbox([entry]);
    const kind = scope === 'staged' ? entry.staged : scope === 'unstaged' ? entry.unstaged : entry.unstaged ?? entry.staged;
    const icon = entry.conflicted ? 'conflict' : entry.untracked ? 'added' : ['added','deleted','renamed'].includes(kind) ? kind : 'modified';
    const diff = scope === 'staged' ? entry.staged_diff : scope === 'unstaged' ? entry.unstaged_diff : entry.diff;
    result.icon = `icons/git-${icon}.svg`;
    Object.assign(result.decoration,{icon_size:14,icon_tone:icon === 'conflict' || icon === 'deleted' ? 'danger' : icon === 'added' ? 'success' : icon === 'renamed' ? 'info' : 'warning',
      detail:view.repo.navigation.hierarchical ? undefined : parent(entry.path),
      badges:[...(diff.additions ? [{text:`+${diff.additions}`,tone:'success'}] : []),...(diff.deletions ? [{text:`−${diff.deletions}`,tone:'danger'}] : [])],
      check:{id:`git-index-${prefix}-${entry.path}`,label:view.text.git_stage_file,checked:selected.checked || prefix === 'staged',mixed:!['staged','unstaged'].includes(prefix) && selected.mixed,disabled:!view.repo.writable() || entry.conflicted}});
  } else if (!row.id.includes('/')) {
    const selected = checkbox(entries);
    result.decoration.check = {id:`git-select-group-${row.id}`,label:view.text.git_stage_file,checked:selected.checked,mixed:selected.mixed,disabled:!view.repo.writable() || !selected.paths.length};
  }
  return result;
}
function changes(view) {
  if (!view.repo.status?.entries.length) return view.repo.status && view.repo.status.kind !== 'directory'
    ? div().flex_1().min_h_0().child(EmptyState.new('git_no_changes',{variant:'list',fill_height:true,icon:'network',label:view.text.git_no_changes})) : empty(view,true);
  view.nodes = new Map();
  const items = rows(view.repo.status,view.repo.navigation,view.text).map(row => decorated(view,row));
  const document = view.repo.documents.get(view.repo.selected), entry = view.repo.status.entries.find(entry => entry.path === document?.request.path);
  const selected = selectedId(entry,view.repo.navigation.grouping,document?.request.scope ?? 'all');
  return div().id('git-change-list').flex_1().min_h_0().p_2().child(ResourceTree.new('git-tree',{items,selected:selected ? [selected] : [],current:selected,indent:14,skip_depth:1}));
}
function historyMetadata(repo,entry,colors) {
  const current = repo.status?.branch;
  const branch = entry.references.find(name => current != null && name === `heads/${current}`)
    ?? entry.references.find(name => name.startsWith('heads/'));
  const fields = [['it/hashtag',entry.id.slice(0,8)],['users/user',entry.author],
    ...(branch ? [['newicons/hierarchy2',branch.slice(6)]] : [])];
  return div().id(`git-history-metadata-${entry.id}`).h_flex().min_w_0().gap_2().overflow_hidden()
    .text_xs().text_color(colors.muted_foreground)
    .children(fields.map(([icon,value]) => div().h_flex().min_w_0().gap_1()
      .child(new Icon(`reicon:${icon}`).size('xsmall').color(colors.muted_foreground))
      .child(div().min_w_0().truncate().child(value))));
}
function history(view) {
  const repo = view.repo, page = repo.history, colors = theme().colors;
  if (page && !page.entries.length) return div().flex_1().min_h_0().child(EmptyState.new('git_history_empty',{variant:'list',fill_height:true,icon:'calendar',label:view.text.git_history_empty}));
  const current = Math.floor((page?.offset ?? 0) / 100) + 1;
  return div().v_flex().flex_1().min_h_0()
    .child(div().id('git-history-list').v_flex().flex_1().min_h_0().overflow_y_scroll().p_2()
      .children(page ? page.entries.map((entry,index) => SelectableRow.new(`git-commit-${entry.id}`,{selected:repo.selected === `commit:${entry.id}`})
        .child(div().relative().h_flex().items_center().justify_center().w_3().min_h_10().self_stretch().flex_shrink(0)
          .child(div().id(`git-history-track-${index}`).absolute().left('50%').top_0().bottom_0().w_px().bg(colors.muted_foreground))
          .child(div().size_2().rounded_full().bg(colors.muted_foreground)))
        .child(div().v_flex().flex_1().min_w_0().gap_1().child(div().truncate().child(entry.message.split('\n')[0]))
          .child(historyMetadata(repo,entry,colors))))
        : [div().p_2().text_sm().text_color(colors.muted_foreground).child(view.text.git_loading)])
      .children(page && (page.references_truncated || page.entries.some(entry => entry.truncated))
        ? [div().p_2().text_xs().text_color(colors.muted_foreground).child(view.text.git_history_partial)] : []))
    .children(page && (page.offset > 0 || page.next) ? [div().h_flex().flex_shrink(0).px_2().justify_between()
      .child(div().text_xs().text_color(colors.muted_foreground).child(`${page.offset + 1}–${page.offset + page.entries.length}`))
      .child(new Pagination('git-history-pages').compact().size('small').total_pages(current + Number(!!page.next)).current_page(current)
        .disabled(repo.historyLoading || !!repo.pending).on_change((page,cx) => view.run(() => repo.historyPage(page),cx)))] : []);
}
function toolbar(view) {
  const repo = view.repo, colors = theme().colors, entries = repo.status?.entries ?? [];
  const additions = entries.reduce((sum,entry) => sum + entry.diff.additions,0), deletions = entries.reduce((sum,entry) => sum + entry.diff.deletions,0);
  const staged = entries.some(entry => entry.staged !== null), unstaged = entries.some(entry => entry.unstaged !== null || entry.untracked);
  const operation = unstaged ? 'stage' : 'unstage', key = unstaged ? 'git_stage' : 'git_unstage';
  return div().h_flex().px_2().py_1().gap_1().flex_shrink(0)
    .child(new Button('git-open-changes').ghost().size('small').label(view.text.git_diff).tooltip(view.text.git_view_diff)
      .disabled(!entries.length).on_click((_,cx) => view.control('git-open-changes',cx)))
    .children(stats(additions,deletions,colors))
    .child(div().flex_1())
    .child(Menu.new('git-stage-split',{label:view.text[key],small:true,items:view.menus.changes().slice(0,2),primary:{id:'git-stage-all',label:view.text[key],enabled:repo.writable() && (staged || unstaged),value:{kind:'index',operation,paths:entries.filter(entry => !entry.conflicted && (operation === 'stage' ? entry.unstaged !== null || entry.untracked : entry.staged !== null)).map(entry => entry.path)}}}));
}
function commit(view) {
  const repo = view.repo, colors = theme().colors;
  return div().id('git-commit-controls').v_flex().flex_shrink(0).p_2().gap_2().border_t_1().border_color(colors.border)
    .children(repo.amend || repo.options.signoff || repo.options.skip_hooks ? [div().h_flex().gap_2().text_xs().text_color(colors.muted_foreground)
      .children(repo.amend ? [new Button('git-amend-active').ghost().size('xsmall').label(view.text.git_amend).on_click((_,cx) => view.amend(cx))] : [])
      .children(repo.options.signoff ? [view.text.git_signoff] : []).children(repo.options.skip_hooks ? [view.text.git_skip_hooks] : [])] : [])
    .child(div().relative().pr_6().child(TextField.new(view.message,{appearance:false,bordered:false,height:repo.commitOpen ? 220 : 86}))
      .child(div().absolute().top_0().right_0().child(IconButton.new('git-expand-commit',{icon:'maximize',label:view.text.git_expand_commit}))))
    .child(Menu.new('git-commit-split',{label:view.text.git_commit,form:true,items:view.menus.commitSplit(),primary:{id:'git-commit',label:view.text.git_commit,icon:'check',enabled:repo.canCommit(),value:{kind:'commit'}}}));
}
export function details(view) {
  const repo = view.repo, colors = theme().colors;
  return div().id('git-changes').v_flex().size_full().min_h_0().min_w_0()
    .child(PanelHeader.new('git-changes-header',{gap:0})
      .child(IconButton.new('git-branch-menu',{icon:'network',label:repo.status?.branch ?? view.text.git_branches,show_label:true,dropdown_caret:true,disabled:!repo.branches}).max_w_full().justify_start())
      .child(div().flex_1()).child(Menu.new('git-actions',{label:view.text.git_actions,icon:'ellipsis',disabled:!repo.status || !repo.branches,items:view.menus.root()}))
      .child(IconButton.new('git-refresh',{icon:'rotate-cw',label:view.text.git_refresh})))
    .child(div().px_2().pt_2().pb_1().child(SegmentedTabs.new('git-views',{selected:repo.view,
      items:[{id:'changes',label:view.text.git_view_changes},{id:'history',label:view.text.git_view_history}]})))
    .children(repo.view === 'changes' ? [toolbar(view)] : [])
    .child(repo.view === 'changes' ? changes(view) : history(view))
    .children(repo.pending && !repo.pending.busy ? [div().h_flex().gap_2().px_2().py_1()
      .child(new Button('git-check-result').size('small').label(view.text.git_check_result).on_click((_,cx) => view.run(() => repo.complete(),cx)))
      .children(repo.pending.error === 'git_branch_conflict' ? [new Button('git-stash-switch').size('small').label(view.text.git_stash_switch).on_click((_,cx) => view.run(() => view.actions.stashSwitch(),cx))] : [])] : [])
    .children(repo.view === 'changes' && repo.status && repo.status.kind !== 'directory' ? [commit(view)] : []);
}
export function dialogs(view) {
  const {dialog,picker} = view.actions, text = view.text, colors = theme().colors;
  const form = dialog?.form, branch = ['branch','rename'].includes(form), prefix = form === 'rename' ? 'git-rename' : 'git-create';
  const confirmation = dialog?.action === 'undo_commit' ? 'git_undo_confirm' : dialog?.action?.push ? 'git_force_push_confirm'
    : dialog?.action?.discard ? 'git_discard_confirm' : dialog?.action?.discard_all ? 'git_discard_all_confirm'
    : dialog?.action?.remove_remote ? 'git_remove_remote_confirm' : dialog?.operation === 'merge' ? 'git_merge_branch_confirm' : 'git_delete_confirm';
  return div().children([Picker.new(`git-picker-${picker?.id ?? 0}`, {open:!!picker,title:picker?.title ?? '',items:picker?.items ?? [],empty:text.git_no_matches}),
    Modal.new(`git-dialog-${dialog?.id ?? 0}`,{open:!!dialog}).children(dialog ? [div().id('git-form').v_flex().w(Math.min(420,window.viewport_size().width - 48)).gap_3()
      .child(div().text_lg().font_semibold().child(text[dialog.title] ?? dialog.title))
      .children(form === 'confirm' ? [div().text_sm().child(`${text[confirmation]}\n${dialog.target}`)] : [])
      .children(branch ? [div().v_flex().gap_2().child(div().text_sm().font_medium().child(text[form === 'rename' ? 'git_branch_source' : 'git_branch_base']))
        .child(div().text_sm().text_color(colors.muted_foreground).truncate().child([dialog.base,dialog.commit?.slice(0,8)].filter(Boolean).join(' · ')))] : [])
      .children(dialog.input ? [div().v_flex().gap_2().child(div().text_sm().font_medium().child(text[form === 'remote' ? 'git_remote_name' : form === 'tag' ? 'git_name' : 'git_branch_name']))
        .child(div().id(branch ? `${prefix}-name` : 'git-form-name').child(TextField.new(dialog.input,{disabled:dialog.busy})))] : [])
      .children(dialog.extra ? [div().v_flex().gap_2().child(div().text_sm().font_medium().child(text[form === 'remote' ? 'git_remote_url' : 'git_tag_message']))
        .child(TextField.new(dialog.extra,{disabled:dialog.busy}))] : [])
      .child(div().h_flex().justify_end().gap_2().mt_3()
        .child(new Button('git-form-cancel').label(text.settings_cancel).on_click((_,cx) => { view.actions.close(); cx.notify(); }))
        .child(new Button('git-form-confirm').primary().label(text[form === 'branch' ? 'git_create_branch' : dialog.title] ?? text.settings_save)
          .disabled(dialog.busy).loading(dialog.busy).on_click((_,cx) => view.run(() => view.actions.submit(),cx))))] : [])]);
}
