const en = {
  title: "Liar's Dice", subtitle: "Three players · Five dice each",
  rule: "No wild dice. Raise quantity, then face. A challenge counts exact matches.",
  names: ["Milo", "Robin", "Quinn", "Ellis", "Sage", "Remy"], you: "You", player: "Player",
  start: "Roll dice", next: "Next round", again: "New match", round: "Round", dice: "dice",
  currentBid: "Current bid", noBid: "No bid", quantity: "Quantity", face: "Face",
  decrease: "Less", increase: "More", bid: "Bid", challenge: "Challenge",
  yourTurn: "Your turn", thinking: "is thinking", waiting: "Waiting for opponent",
  roundOver: "Round complete", matchOver: "Match over", winner: name => `${name} wins`,
  counted: count => `${count} matching dice`, loses: name => `${name} loses a die`,
  bidHeld: "Bid held", bluffCaught: "Bluff caught", eliminated: "Out",
  loading: "Loading", loadFailed: "Could not load game settings", configure: "Configure players in game settings",
  retry: "Retry", failed: "Model request failed", timedOut: "Model response timed out",
  invalidResponse: "Model returned an invalid move", unconfirmed: "AI result unconfirmed · Retry to check",
  invalid: "Choose a higher bid", resetScore: "New match", seconds: "s",
};

const zh = {
  title: "骗子骰子", subtitle: "三人对局 · 每人五颗骰子",
  rule: "没有万能点数。叫点先比数量，再比点数；质疑时只数相同点数。",
  names: ["小满", "阿禾", "小川", "七喜", "木木", "阿南"], you: "你", player: "玩家",
  start: "掷骰子", next: "下一轮", again: "新对局", round: "轮数", dice: "颗骰子",
  currentBid: "当前叫点", noBid: "尚未叫点", quantity: "数量", face: "点数",
  decrease: "减少", increase: "增加", bid: "叫点", challenge: "质疑",
  yourTurn: "轮到你", thinking: "正在思考", waiting: "等待对手",
  roundOver: "本轮结束", matchOver: "对局结束", winner: name => `${name}获胜`,
  counted: count => `实际有 ${count} 颗`, loses: name => `${name}失去一颗骰子`,
  bidHeld: "叫点成立", bluffCaught: "叫点未成立", eliminated: "已淘汰",
  loading: "正在加载", loadFailed: "游戏配置加载失败", configure: "请先设置玩家",
  retry: "重试", failed: "模型请求失败", timedOut: "模型响应超时",
  invalidResponse: "模型返回了无效操作", unconfirmed: "AI 结果未确认，重试以查询",
  invalid: "请选择更高的叫点", resetScore: "新对局", seconds: "秒",
};

export const messages = locale => locale.startsWith("zh") ? zh : en;
