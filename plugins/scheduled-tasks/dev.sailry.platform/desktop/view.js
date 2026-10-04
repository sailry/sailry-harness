import {div} from "gpui-kit";
import {Button, Tab, TabBar} from "gpui-component";
import {theme} from "sailry";
import {CardColumn, CardList, CardRow, CardSummary, EmptyState, IconButton, Modal, SettingsPage, Toggle, Tooltip, openSession} from "sailry/ui";
import {render as editor} from "./editor.js";

function time(timestamp) { return new Date(timestamp).toLocaleString(); }
function status(view, item) {
  if (item.status === "running") return view.text.running;
  if (item.status === "queued") return view.text.queued;
  if (!item.enabled) return view.text.paused;
  if (item.next_ms !== null) return view.text.scheduled;
  const latest = item.status;
  return view.text[latest === "failed" ? "runFailed" : latest] ?? view.text.finished;
}
function card(view, item) {
  const {text} = view, disabled = view.busy || !!view.pending;
  const state = status(view, item);
  return CardRow.new(`task-card-${item.id}`,{row_id:`task-row-${item.id}`})
    .child(CardSummary.new(`task-summary-${item.id}`,{title:item.name,subtitle:view.projectName(item.project),icon:"icons/folder.svg"}))
    .child(CardColumn.new(`task-status-${item.id}`,{variant:"metadata"})
      .child(Tooltip.new(`task-schedule-${item.id}`,{text:item.next_ms === null ? state : `${state} · ${time(item.next_ms)}`})
        .child(div().truncate().child(state))))
    .child(CardColumn.new(`task-leading-${item.id}`,{variant:"control"})
      .child(Toggle.new(`task-enabled-${item.id}`, {label:`${text.enabled}: ${item.name}`,checked:item.enabled,disabled})))
    .child(CardColumn.new(`task-actions-${item.id}`,{variant:"actions"})
      .child(IconButton.new(`task-run-${item.id}`,{icon:"reicon:video/play",label:text.run,disabled}))
      .child(IconButton.new(`task-edit-${item.id}`,{icon:"reicon:newicons/edit",label:text.edit,disabled}))
      .child(IconButton.new(`task-delete-${item.id}`,{icon:"reicon:ui/trash2",label:text.remove,disabled})));
}
function execution(view, item) {
  const {text} = view, {job} = item, colors = theme().colors, disabled = view.busy || !!view.pending;
  const hasActions = !!(job.error || item.session || job.status === "queued" || (job.status === "running" && item.turn));
  return CardRow.new(`task-execution-${job.id}`)
    .child(CardColumn.new(`task-execution-content-${job.id}`,{variant:"content"})
      .child(div().v_flex().gap_1().child(div().truncate().child(item.name))
        .child(CardColumn.new(`task-execution-time-${job.id}`,{variant:"inline"}).child(time(job.created_ms)))))
    .child(CardColumn.new(`task-execution-status-${job.id}`,{variant:"inline"})
      .child(div().text_color(job.status === "completed" ? colors.success : ["failed","unknown"].includes(job.status) ? colors.destructive : colors.muted_foreground)
        .child(text[job.status === "failed" ? "runFailed" : job.status])))
    .children(hasActions ? [CardColumn.new(`task-execution-actions-${job.id}`,{variant:"actions",spacing:"regular"})
      .children(job.error ? [Tooltip.new(`task-error-detail-${job.id}`,{text:job.error.message})
        .child(new Button(`task-details-${job.id}`).ghost().label(text.details))] : [])
      .children(item.session ? [new Button(`task-open-${job.id}`).label(text.open)
        .on_click(() => openSession(item.session))] : [])
      .children(job.status === "queued" || (job.status === "running" && item.turn) ? [new Button(`task-stop-${job.id}`).label(text.stop).disabled(disabled)
        .on_click((_,cx) => view.perform(job.status === "queued" ? "cancel" : "stop",{id:job.id},cx))] : [])] : []);
}
function confirmation(view) {
  const {text} = view;
  return div().id("task-delete-confirmation").v_flex().w(Math.min(480,window.viewport_size().width - 88)).gap_3()
    .child(div().text_lg().font_semibold().child(text.deleteTitle))
    .child(div().truncate().child(view.deleting.name))
    .child(div().text_sm().text_color(theme().colors.muted_foreground).child(text.deleteMessage))
    .child(div().h_flex().justify_end().gap_2()
      .child(new Button("task-delete-cancel").label(text.cancel).disabled(view.busy).on_click((_,cx) => view.close(cx)))
      .child(new Button("task-delete-confirm").danger().label(view.pending ? text.retry : text.remove).disabled(view.busy)
        .on_click((_,cx) => view.perform("remove",{id:view.deleting.id,revision:view.deleting.revision},cx))));
}
export function render(view) {
  const {text} = view, colors = theme().colors, disabled = view.busy || !!view.pending;
  const content = view.tab === 1
    ? CardList.new("tasks-runs")
      .children(view.loading && !view.loaded && !view.history.length ? [div().child(text.loading)] : [])
      .children((view.loaded || !view.loading) && !view.history.length ? [EmptyState.new("tasks-runs-empty",{variant:"card",icon:"calendar",label:text.noRuns})] : [])
      .children(view.history.map(item => execution(view,item)))
      .children(view.more ? [new Button("tasks-more").label(text.more).disabled(view.loading)
        .on_click((_,cx) => { view.pages++; view.refresh(cx); })] : [])
    : CardList.new("tasks-items")
      .children(view.loading && !view.loaded && !view.items.length ? [div().child(text.loading)] : [])
      .children((view.loaded || !view.loading) && !view.items.length ? [EmptyState.new("tasks-empty",{variant:"card",icon:"calendar",label:text.empty})] : [])
      .children(view.items.map(item => card(view,item)));
  const body = SettingsPage.new("tasks", {title:text.title, description:text.description})
    .child(div().id("tasks-toolbar").w_full()
      .child(div().id("tasks-controls").h_flex().items_center().justify_between().gap_3().flex_wrap()
        .child(new TabBar("tasks-tabs").variant("segmented").selected_index(view.tab)
          .child(new Tab().label(text.tasks)).child(new Tab().label(text.runs))
          .on_change((index,cx) => { view.tab = index; cx.notify(); }))
        .child(new Button("task-create").primary().label(text.create).disabled(disabled).on_click((_,cx) => view.edit(null,cx)))))
    .children(!view.editing && !view.deleting && !view.busy && view.pending ? [new Button("task-retry").label(text.retry).disabled(view.busy)
      .on_click((_,cx) => view.perform(null,null,cx)),new Button("task-dismiss").label(text.cancel).disabled(view.busy).on_click((_,cx) => view.close(cx))] : [])
    .child(content);
  return div().id("scheduled-tasks-page").size_full().v_flex().min_h_0().min_w_0()
    .text_color(colors.foreground).child(body)
    .child(Modal.new(`task-dialog-${view.dialogId}`,{open:!!(view.editing || view.deleting),dismissable:!view.busy,
      form:!!view.editing,title:view.editing?.draft.id ? text.edit : text.create,width:560})
      .children(view.editing ? editor(view) : view.deleting ? [confirmation(view)] : []));
}
