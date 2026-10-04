const en = {
  create: 'New terminal', close: 'Close', loading: 'Loading', retry: 'Retry', cli: 'CLI',
  empty: 'No terminals', failed: 'Terminal action failed', loadFailed: 'Could not load terminals',
  unknown: 'Terminal result is unconfirmed', stopped: 'Terminal stopped', terminal: 'Terminal',
  refresh: 'Refresh', toolsFailed: 'Could not load CLI tools',
  unavailable: 'CLI is no longer available', launchFailed: 'Terminal could not start',
  tools: {codex:'Codex',claude:'Claude',gemini:'Gemini',agy:'Agy',grok:'Grok',opencode:'OpenCode',kimi:'Kimi'},
};
const zh = {
  create: '新建终端', close: '关闭', loading: '加载中', retry: '重试', cli: 'CLI',
  empty: '暂无终端', failed: '终端操作失败', loadFailed: '无法加载终端',
  unknown: '终端结果尚未确认', stopped: '终端已停止', terminal: '终端', tools:en.tools,
  refresh: '刷新', toolsFailed: '无法读取 CLI',
  unavailable: 'CLI 已不可用', launchFailed: '终端启动失败',
};
export function messages(locale) { return locale === 'zh-CN' ? zh : en; }
