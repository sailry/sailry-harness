export function messages(locale) {
  return locale === 'zh-CN' ? {
    goal:'目标', active:'执行中', paused:'已停止', completed:'已完成', blocked:'受阻', failed:'失败', cleared:'已清理',
    stop:'停止', resume:'恢复', clear:'清理', retry:'重试', empty:'暂无目标',
    error:'目标操作失败', uncertain:'结果未确认，请重试', readFailed:'无法读取目标',
  } : {
    goal:'Goal', active:'Running', paused:'Stopped', completed:'Completed', blocked:'Blocked', failed:'Failed', cleared:'Cleared',
    stop:'Stop', resume:'Resume', clear:'Clear', retry:'Retry', empty:'No goal',
    error:'Goal action failed', uncertain:'Outcome unconfirmed, retry', readFailed:'Could not read the goal',
  };
}
