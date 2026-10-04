const en = {
  title: "Reversi", subtitle: "Capture the board", you: "You", opponent: "AI",
  startBlack: "Play black", startWhite: "Play white", black: "Black", white: "White",
  first: "First", second: "Second", versus: "VS", newGame: "New game", viewBoard: "View board", again: "Play again",
  wins: "Wins", losses: "Losses", draws: "Draws", moves: "Recent actions", lastMove: "Last move",
  noMoves: "No moves yet", yourTurn: "Your turn", thinking: "Thinking", paused: "AI turn paused",
  win: "You win", lose: "You lose", draw: "Draw", seconds: "s", retry: "Retry",
  loading: "Loading", loadFailed: "Could not load game settings",
  configure: "Configure players in game settings", ready: "Choose a side",
  failed: "Model request failed", timedOut: "Model response timed out",
  invalidResponse: "Model returned an invalid move", unconfirmed: "AI result unconfirmed · Retry to check",
  pass: "Pass", passed: name => `${name} passes`,
  playerMove: (name, at) => `${name} · ${at}`,
  cell: (at, stone) => `${at}, ${stone}`,
  empty: "empty", legal: "legal move", flipped: count => `Flipped ${count}`,
};

const zh = {
  title: "黑白棋", subtitle: "翻转棋盘", you: "你", opponent: "AI",
  startBlack: "执黑先行", startWhite: "执白后行", black: "黑棋", white: "白棋",
  first: "先手", second: "后手", versus: "对", newGame: "新对局", viewBoard: "查看棋盘", again: "再来一局",
  wins: "胜", losses: "负", draws: "平", moves: "最近动态", lastMove: "最近落子",
  noMoves: "尚未落子", yourTurn: "轮到你了", thinking: "正在思考", paused: "AI 回合暂停",
  win: "你赢了", lose: "你输了", draw: "平局", seconds: "秒", retry: "重试",
  loading: "正在加载", loadFailed: "游戏配置加载失败",
  configure: "请先设置玩家", ready: "选择执棋方",
  failed: "模型请求失败", timedOut: "模型响应超时",
  invalidResponse: "模型返回了无效落子", unconfirmed: "AI 结果未确认，重试以查询",
  pass: "停着", passed: name => `${name}停着`,
  playerMove: (name, at) => `${name} · ${at}`,
  cell: (at, stone) => `${at}，${stone}`,
  empty: "空位", legal: "可落子", flipped: count => `翻转 ${count} 枚`,
};

export const messages = locale => locale.startsWith("zh") ? zh : en;
