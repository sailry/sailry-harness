import {div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {theme} from 'sailry';
import {PanelHeader, IconButton, Menu, ResourceTree, NativeContextMenu, Toggle, EmptyState} from 'sailry/ui';
import {TextField} from 'sailry/forms';

function search(view) {
  const {text,search} = view, colors = theme().colors;
  const empty = !search.running && search.result && !search.result.matches.length && search.status === 'file_search_empty';
  return div().id('file-search').v_flex().size_full().min_h_0()
    .child(div().v_flex().p_2().gap_2()
      .child(TextField.new(search.query,{size:'small'}))
      .child(TextField.new(search.filter,{size:'small'}))
      .child(div().h_flex().gap_1()
        .child(Toggle.new('file-search-case',{variant:'button',text:'Aa',label:text.file_search_case,checked:search.case_sensitive}))
        .child(Toggle.new('file-search-regex',{variant:'button',text:'.*',label:text.file_search_regex,checked:search.regex}))
        .child(div().flex_1())
        .child(IconButton.new('file-search-run',{icon:search.running ? 'close' : 'search',
          label:search.running ? text.settings_cancel : text.file_search,disabled:!view.explorer.connected}))))
    .children(search.status && !empty ? [div().px_3().pb_2().text_xs().text_color(colors.muted_foreground).child(text[search.status])] : [])
    .child(div().id('file-search-results').v_flex().flex_1().min_h_0().p_2()
      // Kit b79f4ce cannot register handlers inside lazy List row builders.
      // Node bounds results to 200; eager Kit buttons retain native activation.
      .child(div().id('file-search-list').v_flex().size_full().min_h_0().overflow_y_scroll()
        .children(empty ? [EmptyState.new('file_search_empty',{variant:'list',fill_height:true,icon:'search',label:text.file_search_empty})] : [])
        .children((search.result?.matches ?? []).map((match,index) =>
          new Button(`file-search-result-${index}`).ghost().w_full().h('auto').justify_start()
            .child(div().v_flex().w_full().min_w_0()
              .child(div().truncate().child(`${match.path}:${match.line_number}`))
              .child(div().truncate().text_xs().text_color(colors.muted_foreground).child(match.line)))
            .on_click((_,cx) => view.run(cx => view.open(match.path,match.line_number,cx),cx))))));
}

export function render(view) {
  const {text,explorer,actions} = view, colors = theme().colors;
  const page = explorer.pages.get('');
  return div().id('file-explorer').v_flex().size_full().min_w_0().min_h_0()
    .child(PanelHeader.new('file-explorer-header')
      .child(div().h_flex().w_full().min_w_0().gap_2()
        .child(div().flex_1().min_w_0().truncate().child(text.file_tree))
        .child(Menu.new('file-create-menu',{label:text.files_create,icon:'plus',size:'medium',disabled:!explorer.connected,
          items:actions.create(explorer.directory())}))
        .child(IconButton.new('file-search-toggle',{icon:'search',label:text.file_search,size:'medium',selected:!!view.search,disabled:!explorer.connected}))))
    .child(view.search ? search(view) : NativeContextMenu.new('file-tree-root',{items:actions.tree('')})
      .child(div().id('file-tree').v_flex().flex_1().min_h_0()
        .children(page?.error ? [new Button('file-directory-retry').ghost().label(text.refresh)
          .on_click((_,cx) => view.run(cx => explorer.refresh(cx),cx))] : [])
        .child(!page?.entries?.length
          ? page?.error ? div().flex_1().min_h_0()
            : EmptyState.new(page?.loaded ? 'files_directory_empty' : 'files_loading',{
                variant:'list',fill_height:true,icon:'folder',label:text[page?.loaded ? 'files_directory_empty' : 'files_loading']})
          : div().flex_1().min_h_0().p_2().child(ResourceTree.new('files-tree',{items:explorer.items(text,actions.treeMenu()),
              selected:explorer.paths(),current:explorer.current,menu:actions.tree('')})))
        .children(page?.partial ? [div().px_3().pb_2().text_xs().text_color(colors.muted_foreground)
          .child(text.files_listing_partial)] : [])));
}
