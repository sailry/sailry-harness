// Accepted workbench copy from Sailry ef5e82ea.
const en = {
  activity_board_waiting:'Awaiting approval',activity_board_running:'Running',activity_board_empty:'No activity',
  activity_completed:'Completed',activity_failed:'Failed',activity_idle:'Idle',activity_no_project:'Unassigned',
  sessions_unassigned:'Unassigned',chat_new:'New conversation',turn_queued:'Queued',chat_stopping:'Stopping',
  activity_approval:'Awaiting approval',activity_input:'Awaiting reply',activity_running:'Running',
  chat_interrupted:'Interrupted',turn_cancelled:'Stopped',terminal:'Terminal',live_disconnected:'Disconnected',
  activity_read_failed:'Could not load activity',retry:'Retry',
};
const zh = {
  activity_board_waiting:'待审批',activity_board_running:'运行中',activity_board_empty:'暂无活动',
  activity_completed:'已完成',activity_failed:'失败',activity_idle:'空闲',activity_no_project:'未归类',
  sessions_unassigned:'未归类',chat_new:'新建会话',turn_queued:'等待处理',chat_stopping:'正在停止',
  activity_approval:'待批准',activity_input:'待回复',activity_running:'进行中',
  chat_interrupted:'已中断',turn_cancelled:'已停止',terminal:'终端',live_disconnected:'已断开',
  activity_read_failed:'无法加载活动',retry:'重试',
};
export const messages = locale => locale === 'zh-CN' ? zh : en;
