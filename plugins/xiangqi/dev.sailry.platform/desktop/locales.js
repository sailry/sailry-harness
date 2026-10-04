const glyphs = {
  K: "帥", A: "仕", E: "相", H: "馬", R: "車", C: "炮", P: "兵",
  k: "將", a: "士", e: "象", h: "馬", r: "車", c: "砲", p: "卒",
};

const en = {
  title: "Chinese chess", subtitle: "Xiangqi", you: "You", opponent: "AI",
  red: "Red", black: "Black", first: "First", second: "Second",
  startRed: "Play red", startBlack: "Play black", chooseSide: "Choose a side",
  newGame: "New game", again: "Play again", wins: "Wins", losses: "Losses", draws: "Draws",
  moves: "Moves", noMoves: "No moves yet", lastMove: "Last move",
  yourTurn: "Your turn", thinking: "AI is thinking", paused: "AI turn paused",
  win: "You win", lose: "AI wins", draw: "Draw", check: "Check",
  checkmate: "Checkmate", stalemate: "No legal moves", repetition: "Threefold repetition",
  practiceRule: "Practice rule: the same board and turn three times draws",
  seconds: "s", retry: "Retry", loading: "Loading",
  loadFailed: "Could not load game settings", configure: "Configure players in game settings",
  ready: "Choose a side", failed: "Model request failed", timedOut: "Model response timed out",
  invalidResponse: "Model returned an invalid move", unconfirmed: "AI result unconfirmed · Retry to check",
  glyphs, pieceNames: { k: "general", a: "advisor", e: "elephant", h: "horse",
    r: "chariot", c: "cannon", p: "pawn" },
  cell: (at, piece) => `${at}, ${piece}`, empty: "empty",
  playerMove: (name, from, to, piece) => `${name} · ${piece} ${from}–${to}`,
};

const zh = {
  title: "中国象棋", subtitle: "象棋对弈", you: "你", opponent: "AI",
  red: "红方", black: "黑方", first: "先手", second: "后手",
  startRed: "执红先行", startBlack: "执黑后行", chooseSide: "选择执棋方",
  newGame: "新对局", again: "再来一局", wins: "胜", losses: "负", draws: "平",
  moves: "走棋记录", noMoves: "尚未走棋", lastMove: "最近走棋",
  yourTurn: "轮到你了", thinking: "AI 正在思考", paused: "AI 回合暂停",
  win: "你赢了", lose: "AI 获胜", draw: "平局", check: "将军",
  checkmate: "将死", stalemate: "无棋可走", repetition: "三次重复",
  practiceRule: "练习规则：相同棋盘及行棋方出现三次判和",
  seconds: "秒", retry: "重试", loading: "正在加载",
  loadFailed: "游戏配置加载失败", configure: "请先设置玩家",
  ready: "选择执棋方", failed: "模型请求失败", timedOut: "模型响应超时",
  invalidResponse: "模型返回了无效走法", unconfirmed: "AI 结果未确认，重试以查询",
  glyphs, pieceNames: { k: "将帅", a: "士仕", e: "象相", h: "马",
    r: "车", c: "炮", p: "兵卒" },
  cell: (at, piece) => `${at}，${piece}`, empty: "空位",
  playerMove: (name, from, to, piece) => `${name} · ${piece} ${from}–${to}`,
};

export const messages = locale => locale?.startsWith("zh") ? zh : en;
