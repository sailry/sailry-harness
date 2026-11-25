// Shared by the Node callbacks and the desktop panel: the labels of states and
// pause reasons, and the compact forms of time and token counts. The Node
// hands the label objects to every controller; the panel picks its locale.
const label=(en,zh)=>({label:en,locales:{'zh-CN':zh}});

export const labels={
  goal:label('Goal','目标'),
  running:label('Running','执行中'),
  waiting:label('Waiting','等待中'),
  verifying:label('Verifying','核验中'),
  paused:label('Paused','已暂停'),
  blocked:label('Blocked','受阻'),
  completed:label('Completed','已完成'),
  turns:label('Turns','轮次'),
  time:label('Time','用时'),
  tokens:label('Tokens','Token'),
};

export const reasons={
  stopped:label('Stopped','已停止'),
  user:label('Paused by you','已手动暂停'),
  turns:label('Turn limit reached','已达轮次上限'),
  tokens:label('Token budget reached','已达 Token 预算'),
  time:label('Time limit reached','已达时间上限'),
  no_progress:label('No progress','无进展'),
  needs_you:label('Needs you','需要处理'),
  error:label('Error','出错'),
  restart:label('App restarted','应用已重启'),
  plan:label('Plan mode','计划模式'),
  restored:label('Restored from history','已从历史恢复'),
};

const headings={
  completed:label('Goal completed','目标已完成'),
  paused:label('Goal paused','目标已暂停'),
  blocked:label('Goal blocked','目标受阻'),
};

/** A notification heading, with the pause reason after it. */
export function heading(kind,reason=null) {
  const base=headings[kind],extra=reason ? reasons[reason] : null;
  if (!extra) return {heading:base.label,locales:base.locales,persistent:false};
  return {heading:`${base.label} · ${extra.label}`,persistent:false,
    locales:Object.fromEntries(Object.entries(base.locales).map(([locale,text])=>[locale,`${text} · ${extra.locales[locale]}`]))};
}

/** The shown state of a goal: a sub-state while active, its status otherwise. */
export const phase=goal=>goal.status!=='active' ? goal.status : goal.claim ? 'verifying' : goal.waiting ? 'waiting' : 'running';

export function duration(ms) {
  const minutes=Math.floor(ms/60000);
  if (minutes<1) return '<1m';
  return minutes<60 ? `${minutes}m` : `${Math.floor(minutes/60)}h ${minutes%60}m`;
}

export function count(value) {
  if (value<1000) return String(value);
  if (value<1000000) return `${(value/1000).toFixed(1)}k`;
  return `${(value/1000000).toFixed(1)}M`;
}

/** A tool result: the goal for the model and its caption for the transcript. */
export function present(goal) {
  if (!goal) return {goal:null};
  return {goal,sailry_content:{version:1,blocks:[{kind:'notice',message:labels[goal.state]},{kind:'text',path:'/goal/objective'}]}};
}
