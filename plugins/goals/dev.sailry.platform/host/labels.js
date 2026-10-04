const labels={
  active:{label:'Running',locales:{'zh-CN':'执行中'}},
  paused:{label:'Stopped',locales:{'zh-CN':'已停止'}},
  completed:{label:'Completed',locales:{'zh-CN':'已完成'}},
  blocked:{label:'Blocked',locales:{'zh-CN':'受阻'}},
  failed:{label:'Failed',locales:{'zh-CN':'失败'}},
};
export function present(goal) {
  if (!goal) return {goal:null};
  const {after,...record}=goal;
  return {goal:record,sailry_content:{version:1,blocks:[{kind:'notice',message:labels[goal.state]},{kind:'text',path:'/goal/description'}]}};
}
