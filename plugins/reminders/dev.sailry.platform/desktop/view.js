import { div } from "gpui-kit";
import { Button, Tab, TabBar } from "gpui-component";
import { theme } from "sailry";
import { CardColumn, CardList, CardRow, CardSummary, EmptyState, IconButton, Modal, SelectField, SettingsPage, Toggle } from "sailry/ui";
import { render as editor } from "./editor.js";

function card(view, item) {
  const {text} = view, disabled = view.busy || !!view.pending;
  const date = item.due_ms === null ? "" : new Date(item.due_ms).toLocaleString();
  const row = CardRow.new(`reminder-card-${item.id}`, {row_id:`reminder-row-${item.id}`});
  if (item.completed) row.line_through();
  return row
    .child(CardSummary.new(`reminder-summary-${item.id}`, {title:item.title,subtitle:view.projectName(item.project),icon:"icons/folder.svg"})
      .child(Toggle.new(`reminder-complete-${item.id}`, {variant:"checkbox", label:`${text.complete}: ${item.title}`,
        checked:item.completed, disabled})))
    .child(CardColumn.new(`reminder-time-${item.id}`, {variant:"wide_metadata"})
      .child([date,item.notified_ms === null ? "" : text.notified].filter(Boolean).join(" · ")))
    .child(CardColumn.new(`reminder-actions-${item.id}`, {variant:"actions"})
      .child(IconButton.new(`reminder-edit-${item.id}`, {icon:"reicon:newicons/edit",label:text.edit,disabled}))
      .child(IconButton.new(`reminder-delete-${item.id}`, {icon:"reicon:ui/trash2",label:text.remove,disabled})));
}
function confirmation(view) {
  const {text} = view;
  return div().id("reminder-delete-confirmation").v_flex().w(Math.min(480, window.viewport_size().width - 88)).gap_3()
    .child(div().text_lg().font_semibold().child(text.deleteTitle))
    .child(div().truncate().child(view.deleting.title))
    .child(div().text_sm().text_color(theme().colors.muted_foreground).child(text.deleteMessage))
    .child(div().h_flex().justify_end().gap_2()
      .child(new Button("reminder-delete-cancel").label(text.cancel).disabled(view.busy)
        .on_click((_, cx) => view.close(cx)))
      .child(new Button("reminder-delete-confirm").danger().label(view.pending ? text.retry : text.remove).disabled(view.busy)
        .on_click((_, cx) => view.perform("remove", {id:view.deleting.id,revision:view.deleting.revision}, cx))));
}
function projectFilter(view) {
  const {text} = view;
  const projects = new Map(view.catalog.projects.map(project => [project.id, project.name]));
  for (const item of view.items) if (item.project !== null && !projects.has(item.project)) {
    projects.set(item.project, text.unavailableProject);
  }
  return SelectField.new("reminders-project-filter", {label:text.project, placeholder:text.allProjects,
    selected:view.projectFilter ?? "none", items:[{id:"all",label:text.allProjects}, {id:"none",label:text.noProject},
      ...Array.from(projects, ([id,label]) => ({id,label}))]});
}
export function render(view) {
  const {text} = view, colors = theme().colors, disabled = view.busy || !!view.pending;
  const items = view.items.filter(item => (view.completed === null || item.completed === view.completed)
    && (view.projectFilter === "all" || item.project === view.projectFilter));
  const body = SettingsPage.new("reminders", {title:text.pageTitle, description:text.description})
    .child(div().id("reminders-toolbar").h_flex().items_center().justify_between().gap_3().flex_wrap()
      .child(div().id("reminders-filters").h_flex().items_center().gap_3().min_w_0().flex_1().flex_wrap()
        .child(new TabBar("reminders-filter").variant("segmented").selected_index(view.completed === null ? 0 : view.completed ? 2 : 1)
          .child(new Tab().label(text.all)).child(new Tab().label(text.pending)).child(new Tab().label(text.completed))
          .on_change((index, cx) => { view.completed = [null,false,true][index]; cx.notify(); }))
        .child(div().w_48().max_w_full().child(projectFilter(view))))
      .child(new Button("reminder-create").primary().label(text.create).disabled(disabled)
        .on_click((_, cx) => view.edit(null, cx))))
    .children(!view.editing && !view.deleting && !view.busy && view.pending ? [new Button("reminder-retry").label(text.retry)
      .disabled(view.busy).on_click((_, cx) => view.perform(null, null, cx)),
      new Button("reminder-dismiss").label(text.cancel).disabled(view.busy).on_click((_, cx) => view.close(cx))] : [])
    .child(CardList.new("reminders-items")
      .children(view.loading && !view.loaded && !view.items.length ? [div().child(text.loading)] : [])
      .children((view.loaded || !view.loading) && !items.length ? [EmptyState.new("reminders-empty", {variant:"card",icon:"bell",label:text.empty})] : [])
      .children(items.map(item => card(view, item))));
  return div().id("reminders-page").size_full().v_flex().min_h_0().min_w_0()
    .text_color(colors.foreground).child(body)
    .child(Modal.new(`reminder-dialog-${view.dialogId}`, {open:!!(view.editing || view.deleting), dismissable:!view.busy,
      form:!!view.editing, title:view.editing?.draft.id ? text.edit : text.create, width:480})
      .children(view.editing ? editor(view) : view.deleting ? [confirmation(view)] : []));
}
