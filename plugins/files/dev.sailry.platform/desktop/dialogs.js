import {div} from 'gpui-kit';
import {Button, Progress} from 'gpui-component';
import {theme} from 'sailry';
import {Modal} from 'sailry/ui';
import {TextField} from 'sailry/forms';
import {status} from './transfers.js';
import {parent} from './explorer.js';

export function mutation(view) {
  const {editing,text} = view;
  const operation = editing?.operation;
  const title = operation?.kind === 'rename' ? 'files_rename' : operation?.kind === 'trash' ? 'files_trash'
    : operation?.directory ? 'files_new_directory' : 'files_new_file';
  const action = operation?.pending ? 'files_check_result' : operation?.kind === 'rename' ? 'files_rename'
    : operation?.kind === 'trash' ? 'files_trash' : 'files_create';
  const detail = operation?.paths.length > 1 ? text.files_trash_confirm_many.replace('%{count}',String(operation.paths.length))
    : text.files_trash_confirm.replace('%{name}',operation?.paths[0]?.split('/').at(-1) ?? '');
  return Modal.new(`files-entry-${editing?.id ?? 0}`,{open:!!editing}).children(editing ? [
    div().id('file-entry-dialog').v_flex().w(Math.min(420,window.viewport_size().width - 48)).gap_3()
      .child(div().text_lg().font_semibold().child(text[title]))
      .children(operation.kind === 'trash' ? [div().text_sm().child(detail)]
        : [div().id(operation.kind === 'rename' ? 'file-rename-name' : 'file-create-name')
          .child(TextField.new(editing.input,{disabled:operation.busy || !!operation.pending}))])
      .child(div().h_flex().justify_end().gap_2().mt_3()
        .child(new Button('file-entry-cancel').label(text.settings_cancel).on_click((_,cx) => view.closeEdit(cx)))
        .child((operation.kind === 'trash' ? new Button('file-entry-confirm').danger()
          : new Button('file-entry-confirm').primary()).label(text[action]).disabled(operation.busy)
          .loading(operation.busy).on_click((_,cx) => view.submit(cx))))
  ] : []);
}

export function transfer(view) {
  const {text,transfers} = view, value = transfers.value(), colors = theme().colors;
  const title = value?.kind === 'upload' ? 'files_upload' : value?.kind === 'download' ? 'files_download'
    : value?.kind === 'move' ? 'files_move' : 'files_paste';
  const key = value ? status(value,text) : null;
  const entry = ['copy','move'].includes(value?.kind);
  const editable = value?.stage === 'ready' && entry;
  const pending = ['preparing','transferring','publishing','recycling'].includes(value?.stage);
  const showProgress = pending || value?.stage === 'done' && !entry;
  const progress = value?.stage === 'done' ? 100 : value?.stage === 'transferring' && value.progress
    ? value.progress.size ? value.progress.copied / value.progress.size * 100 : 100 : null;
  const remaining = transfers.queue?.paths ?? [];
  const action = value?.can_check ? 'check' : value?.can_replace ? 'replace' : value?.can_start ? 'start' : null;
  const label = action === 'check' ? 'files_check_result' : action === 'replace' ? 'files_upload_replace'
    : value?.stage === 'failed' ? 'files_upload_retry' : value?.kind === 'move' ? 'files_move'
    : value?.kind === 'upload' ? 'files_upload' : 'files_paste';
  return Modal.new(`files-transfer-${value?.id ?? 'closed'}`,{open:!!value}).children(value ? [
    div().id('file-transfer-dialog').v_flex().w(Math.min(460,window.viewport_size().width - 48)).gap_3()
      .child(div().text_lg().font_semibold().child(text[title]))
      .children(entry ? [
        div().v_flex().gap_1().text_sm().child(div().text_color(colors.muted_foreground).child(text.files_paste_source))
          .child(div().whitespace_normal().child(value.source_label)),
        div().text_sm().text_color(colors.muted_foreground).whitespace_normal().child(value.source_path),
        ...(remaining.length ? [div().id('file-batch-remaining').v_flex().max_h_32().overflow_y_scroll()
          .children(remaining.map(item => div().text_sm().text_color(colors.muted_foreground).child(item.path)))] : []),
        div().v_flex().gap_1().text_sm().child(div().text_color(colors.muted_foreground).child(text.files_paste_directory))
          .child(div().whitespace_normal().child(parent(value.path) || text.files_worktree_root))] : [])
      .children(entry && view.transferInput ? [TextField.new(view.transferInput,{disabled:!editable || transfers.busy})]
        : [div().truncate().text_sm().child(value.path)])
      .children(value.kind === 'upload' ? [div().text_sm().text_color(colors.muted_foreground).child(value.source_path)] : [])
      .children(value.kind === 'move' && value.stage === 'ready' ? [div().text_sm().text_color(colors.muted_foreground)
        .child(text.files_move_recycle)] : [])
      .children(showProgress ? [new Progress('file-transfer-progress').accessibility_label(text[title])
        .loading(progress === null).value(progress ?? 0)] : [])
      .children(key && !['failed','uncertain','done','cancelled'].includes(value.stage) ? [div().id(`file-transfer-status-${value.stage}`).text_sm()
        .text_color(value.error || value.stage === 'uncertain' ? colors.destructive : colors.muted_foreground).child(text[key])] : [])
      .child(div().h_flex().justify_end().gap_2().mt_3()
        .child(new Button('file-transfer-close').label(value.can_cancel ? text.settings_cancel : text.close)
          .on_click((_,cx) => view.run(cx => transfers.close(cx),cx)))
        .children(action ? [new Button('file-transfer-confirm').primary().label(text[label]).disabled(transfers.busy)
          .on_click((_,cx) => view.transfer(action,cx))] : []))
  ] : []);
}
