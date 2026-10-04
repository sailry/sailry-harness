import {div} from 'gpui-kit';
import {Button, Tab, TabBar} from 'gpui-component';
import {Terminal, theme} from 'sailry';
import {Menu} from 'sailry/ui';

export function render(view) {
  const {text} = view, disabled = view.busy || !!view.pending;
  const selected = view.items.find(item => item.id === view.selected);
  const attached = selected && selected.status.kind !== 'stopped';
  const stoppedShell = selected?.status.kind === 'stopped' && !selected.tool && !selected.ssh;
  return div().id('commands-page').v_flex().size_full().min_w_0().min_h_0()
    .children(!view.resource ? [div().h_flex().flex_shrink(0).gap_1().px_2().py_1()
      .child(new Button('terminal-new').ghost().label(text.create).disabled(disabled)
        .on_click((_, cx) => view.perform('create', null, cx)))
      .child(Menu.new('terminal-tools', {label:text.cli, disabled,
        items:view.tools.map(entry => ({id:entry.tool,label:text.tools[entry.tool],enabled:entry.available}))}))
      .child(div().flex_1())
      .children(selected ? [new Button('terminal-close').ghost().label(text.close).disabled(disabled)
        .on_click((_, cx) => view.perform('close', selected.id, cx))] : [])] : [])
    .children(!view.resource && view.items.length > 1 ? [new TabBar('terminal-tabs').variant('underline')
      .selected_index(view.items.findIndex(item => item.id === view.selected))
      .children(view.items.map((item, index) => new Tab().label(view.label(item, index))))
      .on_change((index, cx) => view.select(index, cx))] : [])
    .children(view.error && (view.pending || view.openRetry) ? [div().h_flex().gap_2().px_3().py_2()
      .child(new Button('terminal-retry').ghost().label(text.retry).disabled(view.busy)
        .on_click((_, cx) => view.retry(cx)))] : [])
    .children(!attached && (!stoppedShell || !view.error) ? [div().flex_1().v_flex().items_center().justify_center()
      .child(stoppedShell ? text.loading : selected ? text.stopped : text.empty)] : [])
    .child(div().id(`terminal-content-${selected?.status.kind ?? 'empty'}`)
      .flex_grow(attached ? 1 : 0).min_h_0().min_w_0().children([
      Terminal.new('commands-terminal', {terminal:attached ? selected.id : null}),
    ]));
}
