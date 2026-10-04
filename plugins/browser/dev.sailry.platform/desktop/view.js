// Browser chrome follows Sailry c4bb60a2's workspace/view.rs and shared tab strip.
import {div} from 'gpui-kit';
import {Icon} from 'gpui-component';
import {BrowserSurface, theme} from 'sailry';
import {PanelHeader, NavigationTabs, IconButton} from 'sailry/ui';
import {TextField} from 'sailry/forms';

function empty(text, colors) {
  return div().size_full().min_h_0().v_flex().items_center().justify_center().p_6()
    .child(div().id('empty-browser_empty').v_flex().w_full().max_w_80().items_center().gap_4().text_center()
      .child(div().id('empty-icon-browser_empty').size_16().flex_shrink(0).h_flex().items_center().justify_center()
        .rounded_full().bg(colors.muted).text_color(colors.foreground)
        .child(new Icon('icons/globe.svg').size_7()))
      .child(div().id('empty-title-browser_empty').text_lg().line_height(1.4).font_semibold()
        .text_color(colors.foreground).child(text.browser_empty)));
}

export function render(view) {
  const {text,state} = view, tab = view.selected(), colors = theme().colors;
  if (!tab) return div();
  const button = (id, icon, label, disabled = false) => IconButton.new(id, {icon,label,disabled});
  return div().id('browser-panel').v_flex().size_full().min_h_0().min_w_0()
    .child(PanelHeader.new('browser-header',{padding:8,surface:'content'})
      .child(div().h_flex().w_full().min_w_0().gap_1()
        .child(NavigationTabs.new('browser-tabs', {
          items:state.tabs.map(item => ({id:String(item.id),label:item.title || text.browser_new_tab,
            icon:'globe',loading:item.loading,closable:true})),
          selected:String(state.selected),max_width:180,close_label:text.close
        }))
        .child(button('browser-new-tab','plus',text.browser_new_tab))))
    .child(div().h_flex().gap_1().p_2().border_b_1().border_color(colors.border).flex_shrink(0)
      .child(button('browser_back','arrow-left',text.browser_back,!state.history[0]))
      .child(button('browser_forward','arrow-right',text.browser_forward,!state.history[1]))
      .child(button('browser_reload',tab.loading ? 'close' : 'rotate-cw',text.browser_reload,!tab.url))
      .child(div().id('browser-address').flex_1().min_w_0().child(TextField.new(view.address)))
      .child(button('browser-go','arrow-right',text.browser_go))
      .child(button('browser-settings','settings',text.settings_browser)))
    .child(div().id('browser-content').relative().flex_1().min_h_0().min_w_0()
      .child(BrowserSurface.new('browser-surface'))
      .children(tab.loaded ? [] : [div().absolute().inset_0().child(empty(text,colors))]));
}
