// Document composition retained from Sailry 116aab0f; the surfaces own editing.
import {div} from 'gpui-kit';
import {StatusBar, Tab, TabBar} from 'gpui-component';
import {Header,theme} from 'sailry';
import {DocumentSurface} from 'sailry/documents';
import {PanelHeader, NavigationTabs, IconButton, NativeContextMenu, EmptyState} from 'sailry/ui';

export function header(view) {
  const {documents,text} = view;
  const tabs = {
      id:'file-tabs',
      items:documents.items().map(document => ({id:document.id,
        label:document.path.split('/').at(-1),dirty:document.dirty,closable:true,
        close_label:`${text.close} ${document.path}`})),
      selected:documents.selected,max_width:180,close_label:text.close
    };
  if (view.mode === 'main') return Header.new('files-header',{content:JSON.stringify({title:text.files,tabs})});
  return PanelHeader.new('file-tabs-header',{padding:8}).child(div().w_full().min_w_0()
    .child(NavigationTabs.new(tabs.id,tabs)));
}

function separator(colors) {
  return div().w_px().h_4().mx_1().flex_shrink(0).bg(colors.border);
}

function toolbar(view, document, colors) {
  const {text} = view;
  const button = (id,icon,disabled) => IconButton.new(id,{icon,label:text[id],disabled});
  const actions = [
    ['files_undo','undo',!document.can_edit],
    ['files_redo','redo',!document.can_edit],
    ['files_copy','copy',!document.can_copy],
    ['files_cut','cut',!document.can_cut_paste],
    ['files_paste','paste',!document.can_cut_paste]
  ];
  return div().id('file-toolbar').h_flex().w_full().h_12().px_3().gap_1().flex_shrink(0)
    .border_b_1().border_color(colors.border)
    .child(IconButton.new('files_save',{icon:'circle-check',label:text.files_save,
      disabled:!document.can_save,tone:document.dirty && !document.saving ? 'success' : 'muted'}))
    .child(separator(colors))
    .child(div().id('file-edit-actions').h_flex().flex_1().min_w_0().gap_1().overflow_x_scroll()
      .children(actions.map(([id,icon,disabled]) => button(id,icon,disabled)))
      .child(separator(colors)).child(button('files_find','search',false)))
    .children(document.markdown ? [new TabBar('file-editor-mode').flex_shrink(0)
      .variant('segmented').size('small').selected_index(document.mode === 'source' ? 1 : 0)
      .child(new Tab().label(text.content_document))
      .child(new Tab().label(text.content_source))
      .on_change((index,cx) => view.run(cx => view.documents.action(index === 0 ? 'document' : 'source',cx),cx))] : []);
}

function footer(view, document, colors) {
  const {text} = view;
  const status = document?.saving ? 'files_saving' : document?.truncated ? 'files_content_partial' : null;
  const path = div().id('document-path-label').flex_1().min_w_0().truncate().child(document?.path ?? '');
  const details = document ? div().id('document-status-details').h_flex().flex_shrink(0).gap_2()
    .child(div().id('document-cursor-position').child(text.files_cursor_position
      .replace('{line}',String(document.position.line)).replace('{column}',String(document.position.column))))
    .child(div().id('document-encoding').child('UTF-8'))
    .child(div().id('document-language').child(document.language)) : div();
  return div().id('document-path').flex_shrink(0).min_w_0()
    .child(new StatusBar().h_8().flex_shrink(0).border_t_0().bg('#00000000')
      .left_content(document ? NativeContextMenu.new(`document-path-${document.id}`,{items:view.pathItems(document)}).child(path) : path)
      .children(status ? [div().truncate().child(text[status])] : [])
      .right_content(details));
}

function empty(text) {
  return EmptyState.new('file_empty',{icon:'file',label:text.file_empty});
}

export function content(view) {
  const document = view.documents.current(), colors = theme().colors;
  return div().id('file-document').v_flex().size_full().min_h_0().min_w_0()
    .children(document ? [toolbar(view,document,colors)] : [])
    .child(div().flex_1().min_h_0().min_w_0()
      .child(document ? DocumentSurface.new(`file-editor-${document.id}`,{document:document.id}) : empty(view.text)))
    .children(document ? [footer(view,document,colors)] : []);
}
