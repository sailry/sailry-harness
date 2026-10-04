// Permission captions from Sailry ab5251de, localized for the package.
const en = {
  computer_permissions:'System permissions',
  computer_screen_capture:'Screen Recording',
  computer_accessibility:'Accessibility',
  computer_permission_granted:'Granted',
  computer_permission_missing:'Not granted',
  computer_permission_unknown:'Cannot detect',
  computer_permissions_checking:'Checking',
  computer_permissions_failed:'Check failed',
  computer_permissions_open_failed:'Could not open System Settings',
  computer_permissions_refresh:'Check again',
  permission_request:'Request access',
  computer_permissions_remote:'Open Sailry settings on the execution computer to grant access',
  computer_permissions_unsupported:'Permission checks and guidance are unavailable on this system',
  plugins_disconnected:'Host disconnected'
};
const zh = {
  computer_permissions:'系统权限',
  computer_screen_capture:'屏幕录制',
  computer_accessibility:'辅助功能',
  computer_permission_granted:'已授权',
  computer_permission_missing:'未授权',
  computer_permission_unknown:'无法检测',
  computer_permissions_checking:'检测中',
  computer_permissions_failed:'检测失败',
  computer_permissions_open_failed:'未能打开系统设置',
  computer_permissions_refresh:'重新检测',
  permission_request:'请求授权',
  computer_permissions_remote:'请在执行电脑上打开 Sailry 设置并授权',
  computer_permissions_unsupported:'此系统暂不支持权限检测与授权引导',
  plugins_disconnected:'主机连接已断开'
};
export function messages(locale) { return locale === 'zh-CN' ? zh : en; }
