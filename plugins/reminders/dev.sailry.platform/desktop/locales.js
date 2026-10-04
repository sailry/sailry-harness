const en = {
  pageTitle: "Reminders", description: "To-dos and scheduled reminders",
  all: "All", pending: "Pending", completed: "Completed", create: "New reminder", edit: "Edit", remove: "Delete",
  title: "Title", message: "Notes", timed: "Remind me", time: "Local time", save: "Save", cancel: "Cancel",
  retry: "Retry", empty: "No reminders", loading: "Loading…", complete: "Complete",
  titlePlaceholder: "What needs doing?", messagePlaceholder: "Add a note", projectPlaceholder: "Choose a project",
  notified: "Notified", deleteTitle: "Delete reminder?",
  project: "Project", noProject: "No project", allProjects: "All projects", unavailableProject: "Unavailable project",
  projectUnavailable: "Project unavailable. Choose another project",
  deleteMessage: "This removes the reminder and its schedule", required: "Enter a title",
  invalidTime: "Choose a valid future time", failed: "Could not save", loadFailed: "Could not load reminders",
  conflict: "Reminder changed. Reopen to edit", unknown: "Result unconfirmed. Retry to check",
};
const zh = {
  pageTitle: "待办提醒", description: "管理待办事项与定时提醒",
  all: "全部", pending: "待办", completed: "完成", create: "新建提醒", edit: "编辑", remove: "删除",
  title: "标题", message: "备注", timed: "定时提醒", time: "本地时间", save: "保存", cancel: "取消",
  retry: "重试", empty: "暂无提醒", loading: "正在加载…", complete: "完成",
  titlePlaceholder: "有什么待办？", messagePlaceholder: "添加备注", projectPlaceholder: "选择项目",
  notified: "已提醒", deleteTitle: "删除提醒？",
  project: "项目", noProject: "无项目", allProjects: "全部项目", unavailableProject: "项目不可用",
  projectUnavailable: "项目不可用，请重新选择项目",
  deleteMessage: "将移除这条提醒及其定时计划", required: "请输入标题",
  invalidTime: "请选择有效的未来时间", failed: "保存失败", loadFailed: "加载提醒失败",
  conflict: "提醒已变更，请重新打开编辑", unknown: "结果尚未确认，请重试核对",
};
export function messages(locale) { return locale.startsWith("zh") ? zh : en; }
