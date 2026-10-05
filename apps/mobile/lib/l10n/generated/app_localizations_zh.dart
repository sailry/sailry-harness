// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get updatesVersion => '版本';

  @override
  String get updatesCheck => '检查更新';

  @override
  String get updatesChecking => '检查中';

  @override
  String updatesAvailable(String version) {
    return '有新版本 $version';
  }

  @override
  String get updatesDownload => '下载更新';

  @override
  String get updatesCurrent => '已是最新版本';

  @override
  String get updatesUnpublished => '暂无移动端发布版本';

  @override
  String get updatesCheckFailed => '无法检查更新';

  @override
  String get updatesOpenFailed => '无法打开下载链接';

  @override
  String get retryTask => '重试';

  @override
  String get welcomeTitle => '今天，想一起做点什么？';

  @override
  String get welcomeExplore => '探索项目';

  @override
  String get welcomeExploreDetail => '了解结构与关键入口';

  @override
  String get welcomeExplorePrompt => '请帮我梳理这个项目的结构，说明关键模块和入口。';

  @override
  String get welcomeBuild => '实现想法';

  @override
  String get welcomeBuildDetail => '把想法做出来';

  @override
  String get welcomeBuildPrompt => '我想为这个项目新增一个功能，请先和我确认需求并制定实现方案。';

  @override
  String get welcomeReview => '审查变更';

  @override
  String get welcomeReviewDetail => '检查改动与潜在问题';

  @override
  String get welcomeReviewPrompt => '请审查当前项目的改动，重点检查潜在问题和缺失的测试。';

  @override
  String get welcomePlan => '制定计划';

  @override
  String get welcomePlanDetail => '理清目标与步骤';

  @override
  String get welcomePlanPrompt => '请帮我为接下来的开发工作制定分步骤的计划。';

  @override
  String get conversationEmpty => '描述你的任务';

  @override
  String get conversationLoading => '正在加载会话';

  @override
  String get conversationReconnecting => '正在重连';

  @override
  String get conversationErrorDetails => '查看原因';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return '正在重试模型请求 $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return '模型请求已重试 $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count 个工具调用';
  }

  @override
  String get conversationGoal => '目标';

  @override
  String get conversationGoalBlocked => '受阻';

  @override
  String conversationGoalBudget(String count) {
    return '预算 $count tokens';
  }

  @override
  String get conversationStepSkipped => '已跳过';

  @override
  String get conversationChild => '子任务';

  @override
  String get conversationChildReadonly => '子任务会话';

  @override
  String get conversationOffline => '连接已断开';

  @override
  String get conversationUnavailable => '会话不可用';

  @override
  String get conversationFailed => '操作失败，请重试';

  @override
  String get conversationUnknown => '结果尚未确认，请查看会话后再操作';

  @override
  String get conversationCheckResult => '确认结果';

  @override
  String get conversationConflict => '配置已更新，请重新打开后再操作';

  @override
  String get conversationOlder => '加载更早的消息';

  @override
  String get conversationNew => '新会话';

  @override
  String get conversationNoHost => '请先连接主机';

  @override
  String get conversationNoProject => '请先在主机添加项目';

  @override
  String get conversationNoModel => '请先在主机配置模型';

  @override
  String get conversationNoTasks => '暂无会话';

  @override
  String get conversationNoMessages => '暂无消息';

  @override
  String get conversationPreviewUnavailable => '消息暂不可用';

  @override
  String get conversationInterrupted => '已中断';

  @override
  String get conversationFailedStatus => '处理失败';

  @override
  String get conversationStopping => '正在停止';

  @override
  String get conversationQueued => '等待处理';

  @override
  String get conversationProcessing => '正在处理';

  @override
  String get conversationUnsynced => '状态未同步';

  @override
  String get conversationGenerating => '正在回复';

  @override
  String get conversationWaiting => '等待确认';

  @override
  String get conversationCompacting => '正在压缩上下文';

  @override
  String get conversationForkConfirm => '从当前记录创建分支会话？';

  @override
  String get conversationCompacted => '上下文已压缩';

  @override
  String get conversationToolWaiting => '等待执行';

  @override
  String get conversationToolRunning => '正在执行';

  @override
  String get conversationToolReturned => '已返回';

  @override
  String get conversationToolCancelled => '已取消';

  @override
  String get conversationToolNotExecuted => '未执行';

  @override
  String get conversationToolInterrupted => '已中断';

  @override
  String get conversationUnsupportedInput => '请在桌面端处理此输入';

  @override
  String get conversationStartCoding => '开始执行';

  @override
  String get conversationPlanFeedback => '修改建议';

  @override
  String get conversationOther => '其他';

  @override
  String get conversationSubmit => '提交';

  @override
  String get conversationSource => '来源';

  @override
  String get conversationMode => '工作模式';

  @override
  String get conversationCode => '执行';

  @override
  String get conversationPlan => '计划';

  @override
  String get conversationPermission => '权限';

  @override
  String get conversationAsk => '每次询问';

  @override
  String get conversationProject => '项目内';

  @override
  String get conversationFull => '完全访问';

  @override
  String get conversationReasoning => '思考强度';

  @override
  String get conversationDefault => '默认';

  @override
  String get conversationNone => '关闭';

  @override
  String get conversationMinimal => '最低';

  @override
  String get conversationLow => '低';

  @override
  String get conversationMedium => '中';

  @override
  String get conversationHigh => '高';

  @override
  String get conversationXHigh => '更高';

  @override
  String get conversationMax => '最高';

  @override
  String get conversationBudget => '思考预算';

  @override
  String get conversationAttachment => '附件';

  @override
  String get conversationAttachmentTooLarge => '附件不可读或超过 64 MB';

  @override
  String get conversationDownload => '查看附件';

  @override
  String get conversationImageFailed => '图片无法显示';

  @override
  String get conversationDownloadFailed => '附件加载失败';

  @override
  String get conversationReadonly => '此会话已归档';

  @override
  String get conversationMicrophoneDenied => '无法访问麦克风';

  @override
  String get conversationRecordingFailed => '语音识别失败';

  @override
  String get conversationSpeechDisabled => '语音输入已关闭';

  @override
  String get conversationSpeechMissing => '请先在设置中下载语音模型';

  @override
  String get conversationRecording => '正在录音';

  @override
  String get conversationTranscribing => '正在识别';

  @override
  String get conversationRecordReady => '准备录音';

  @override
  String get conversationStartRecording => '开始录音';

  @override
  String get conversationFinishRecording => '完成录音';

  @override
  String get conversationSources => '来源';

  @override
  String get conversationSearchSuggestions => '搜索建议';

  @override
  String get conversationStats => '会话用量';

  @override
  String get conversationStatsEmpty => '暂无用量';

  @override
  String get conversationStatsOverview => '概览';

  @override
  String get conversationStatsTokenGroup => 'Token 用量';

  @override
  String get conversationStatsCostGroup => '费用';

  @override
  String get conversationStatsGenerationGroup => '生成';

  @override
  String get conversationStatsTokens => 'Token';

  @override
  String get conversationStatsInput => '输入';

  @override
  String get conversationStatsOutput => '输出';

  @override
  String get conversationStatsCached => '缓存输入';

  @override
  String get conversationStatsReasoning => '思考输出';

  @override
  String get conversationStatsCacheRate => '缓存命中';

  @override
  String get conversationStatsCost => '预估费用';

  @override
  String get conversationStatsCostCoverage => '费用覆盖';

  @override
  String get conversationStatsSpeed => '生成速度';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => '计时覆盖';

  @override
  String get conversationStatsTurns => '轮次';

  @override
  String get conversationStatsResponses => '模型响应';

  @override
  String get conversationStatsContext => '当前上下文';

  @override
  String get conversationStatsInputCost => '输入费用';

  @override
  String get conversationStatsOutputCost => '输出费用';

  @override
  String get conversationStatsCacheReadCost => '缓存读取费用';

  @override
  String get conversationStatsCacheWriteCost => '缓存写入费用';

  @override
  String get messageHistoryUpdated => '会话已更新，原记录已保留';

  @override
  String get turnUndoUnsaved => '文件有未保存的修改，请先保存或放弃';

  @override
  String get codePlain => '纯文本';

  @override
  String get toolArguments => '参数';

  @override
  String get toolResult => '结果';

  @override
  String get toolRaw => '原始结果';

  @override
  String get turnChanges => '本轮变更';

  @override
  String turnChangesCount(String count) {
    return '$count 个文件';
  }

  @override
  String get turnUndo => '撤销修改';

  @override
  String get turnUndoAll => '全部撤销';

  @override
  String get turnUndoConfirm => '撤销这些文件在本轮中的修改？后续修改发生冲突时会停止';

  @override
  String get turnUndoDone => '已撤销';

  @override
  String get turnUndoPartial => '部分修改已撤销，请检查剩余文件';

  @override
  String get messageActions => '消息操作';

  @override
  String get messageEdit => '编辑并重新生成';

  @override
  String get messageEditConfirm => '替换此消息及后续会话？文件不会随会话回退';

  @override
  String get messageRewind => '回退至此';

  @override
  String get messageRewindConfirm => '回退至此轮？后续会话会保留为备份，文件不会回退';

  @override
  String get messageBackup => '查看会话备份';

  @override
  String get messageRegenerate => '重新生成';

  @override
  String get messageSearch => '搜索会话';

  @override
  String get messageSearchHint => '搜索消息';

  @override
  String get messageSearchMissing => '此消息已不在当前会话中';

  @override
  String get messageSearchStale => '会话已变更，请重新搜索';

  @override
  String get messageNoResults => '没有匹配消息';

  @override
  String get messageCheck => '确认操作结果';

  @override
  String get messageReference => '引用';

  @override
  String get messageReferenceContext => '此引用属于该消息发送时的上下文';

  @override
  String get toolFailed => '失败';

  @override
  String toolExitCode(String code) {
    return '退出码 $code';
  }

  @override
  String toolSignal(String signal) {
    return '信号 $signal 终止';
  }

  @override
  String get toolTimedOut => '命令超时';

  @override
  String get toolCancelled => '命令已取消';

  @override
  String get toolOutcomeUnknown => '命令结果未知';

  @override
  String get toolQuestionAnswered => '已回答';

  @override
  String get toolQuestionDeclined => '已拒绝';

  @override
  String get toolQuestionCancelled => '已取消';

  @override
  String get fileLinkUnavailable => '无法打开此链接';

  @override
  String get imagePreview => '图片预览';

  @override
  String get fileOpenExternal => '用其他应用打开';

  @override
  String get fileOpenFailed => '无法打开文件';

  @override
  String get fileNoApplication => '没有可打开此文件的应用';

  @override
  String get fileSaveBeforeShare => '保存修改后分享？';

  @override
  String fileTrashConfirm(String name) {
    return '将“$name”移到主机回收站？未保存的修改也会丢弃';
  }

  @override
  String get fileTrashUncertain => '删除结果待确认，请重试查询';

  @override
  String get fileSaveFailed => '文件保存失败';

  @override
  String get terminalHideKeyboard => '收起键盘';

  @override
  String get terminalEscape => 'Esc';

  @override
  String get terminalTab => 'Tab';

  @override
  String get terminalCtrl => 'Ctrl';

  @override
  String get terminalAlt => 'Alt';

  @override
  String get terminalShift => 'Shift';

  @override
  String get terminalCmd => 'Cmd';

  @override
  String get terminalArrowLeft => '向左';

  @override
  String get terminalArrowUp => '向上';

  @override
  String get terminalArrowDown => '向下';

  @override
  String get terminalArrowRight => '向右';

  @override
  String get resourceNoWorkspace => '请选择已连接主机的工作树';

  @override
  String get resourceDisconnected => '主机未连接';

  @override
  String get resourceRoot => '根目录';

  @override
  String get resourceMore => '加载更多';

  @override
  String get resourcePartial => '仅显示部分内容';

  @override
  String get resourceEmpty => '暂无内容';

  @override
  String get resourceSaveError => '保存失败，草稿已保留';

  @override
  String get resourceReloadConfirm => '放弃草稿并读取最新内容？';

  @override
  String get resourceWorktreeCreate => '新建工作树';

  @override
  String get resourceSessionServices => '会话服务';

  @override
  String get resourceServicesUnavailable => '服务列表暂不可用';

  @override
  String get resourceNoServices => '未发现服务地址';

  @override
  String get resourceServiceOpen => '打开服务';

  @override
  String get resourceRemotePort => '远端端口';

  @override
  String get resourceOpenPort => '转发端口';

  @override
  String get resourceOpenBrowser => '预览网页';

  @override
  String get resourcePreviewFailed => '网页加载失败';

  @override
  String get resourcePreviewLink => '无法在预览中打开此链接';

  @override
  String get resourceForwardStopped => '转发已停止';

  @override
  String get resourceTerminalControl => '接管输入';

  @override
  String get resourceTerminalControlHint => '由另一设备控制';

  @override
  String get resourceTerminalClaiming => '正在接管';

  @override
  String get resourceTerminalReadOnly => '只读终端';

  @override
  String get resourceTerminalEnded => '终端已结束';

  @override
  String get resourceTerminalConnecting => '正在连接终端';

  @override
  String get resourceTerminalInput => '终端输入';

  @override
  String get resourceTerminalPaste => '粘贴';

  @override
  String get resourceGitNotRepository => '此目录不是 Git 仓库';

  @override
  String get resourceInvalidPort => '请输入 1–65535 的端口';

  @override
  String get tool_navigate => '打开页面';

  @override
  String get tool_back => '返回上一页';

  @override
  String get tool_forward => '前进一页';

  @override
  String get tool_refresh => '刷新页面';

  @override
  String get tool_right_click => '点击元素';

  @override
  String get tool_clear => '输入文字';

  @override
  String get tool_select => '选择选项';

  @override
  String get tool_hover => '悬停元素';

  @override
  String get tool_scroll => '滚动页面';

  @override
  String get tool_press_key => '按键';

  @override
  String get tool_new_tab => '新建标签页';

  @override
  String get tool_list_windows => '浏览器标签页';

  @override
  String get tool_switch_window => '切换标签页';

  @override
  String get tool_close_window => '关闭窗口';

  @override
  String get tool_close_session => '关闭浏览器';

  @override
  String get tool_screenshot => '截取页面';

  @override
  String get tool_print_to_pdf => '导出 PDF';

  @override
  String get tool_file_upload => '上传文件';

  @override
  String get tool_downloads => '查看下载';

  @override
  String get tool_save_download => '保存下载文件';

  @override
  String get tool_evaluate_js => '执行页面脚本';

  @override
  String get tool_get_cookies => '读取 Cookie';

  @override
  String get tool_delete_all_cookies => '修改 Cookie';

  @override
  String get tool_drag_and_drop => '拖动元素';

  @override
  String get tool_focus => '聚焦元素';

  @override
  String get tool_handle_alert => '处理页面提示';

  @override
  String get tool_database_catalog => '浏览数据库';

  @override
  String get tool_database_query => '查询数据库';

  @override
  String get tool_database_execute => '执行数据库操作';

  @override
  String get tool_search_memory => '检索记忆';

  @override
  String get tool_review_memories => '整理记忆';

  @override
  String get tool_consolidate_memories => '合并记忆';

  @override
  String get tool_save_memory => '保存记忆';

  @override
  String get tool_forget_memory => '删除记忆';

  @override
  String get tool_update_plan => '更新计划';

  @override
  String get tool_create_goal => '创建目标';

  @override
  String get tool_get_goal => '查看目标';

  @override
  String get tool_update_goal => '更新目标';

  @override
  String get tool_spawn_agent => '子代理';

  @override
  String get tool_browser_tabs => '浏览器标签页';

  @override
  String get tool_browser_read => '读取页面';

  @override
  String get tool_browser_navigate => '打开页面';

  @override
  String get tool_browser_click => '点击元素';

  @override
  String get tool_browser_input => '输入文字';

  @override
  String get tool_browser_scroll => '滚动页面';

  @override
  String get tool_browser_back => '返回上一页';

  @override
  String get tool_browser_forward => '前进一页';

  @override
  String get tool_browser_refresh => '刷新页面';

  @override
  String get tool_browser_open => '新建标签页';

  @override
  String get tool_browser_close => '关闭标签页';

  @override
  String get tool_browser_focus => '切换标签页';

  @override
  String get tool_browser_select => '选择选项';

  @override
  String get tool_browser_hover => '悬停元素';

  @override
  String get tool_browser_key => '按键';

  @override
  String get tool_browser_frame => '切换框架';

  @override
  String get tool_browser_wait => '等待页面';

  @override
  String get tool_browser_screenshot => '截取页面';

  @override
  String get tool_ssh_run => '运行 SSH 命令';

  @override
  String get tool_ssh_transfer => '传输 SSH 文件';

  @override
  String get tool_list_worktrees => '列出工作树';

  @override
  String get tool_create_worktree => '新建工作树';

  @override
  String get tool_register_worktree => '添加工作树';

  @override
  String get tool_remove_worktree => '移除工作树';

  @override
  String get tool_google_search => '网页搜索';

  @override
  String get tool_web_fetch => '网页抓取';

  @override
  String get tool_fetch_url => '网页抓取';

  @override
  String get tool_read_file => '读取文件';

  @override
  String get tool_write_file => '写入文件';

  @override
  String get tool_list_directory => '浏览目录';

  @override
  String get tool_search_files => '搜索文件';

  @override
  String get tool_run_command => '运行命令';

  @override
  String get tool_read_command => '查看后台命令';

  @override
  String get tool_stop_command => '停止命令';

  @override
  String get tool_load_skill => '加载技能';

  @override
  String get tool_read_skill_resource => '读取技能资源';

  @override
  String get tool_computer_desktop => '查看桌面';

  @override
  String get tool_computer_observe => '观察屏幕';

  @override
  String get tool_computer_input => '操作电脑';

  @override
  String get tool_computer_focus => '切换应用';

  @override
  String get tool_computer_open => '打开应用';

  @override
  String get tool_git_status => 'Git 状态';

  @override
  String get tool_git_diff => '查看差异';

  @override
  String get tool_git_log => 'Git 日志';

  @override
  String get tool_inspect_image => '识别图片';

  @override
  String get tool_generate_image => '生成图片';

  @override
  String get tool_generate_video => '生成视频';

  @override
  String get terminalUnavailable => '终端将在接入后可用';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => '刷新';

  @override
  String get loading => '正在加载';

  @override
  String get home => '会话';

  @override
  String get idle => '空闲';

  @override
  String get allProjects => '全部项目';

  @override
  String get allWorktrees => '全部工作树';

  @override
  String get filterProjects => '筛选项目';

  @override
  String get closeSearch => '收起搜索';

  @override
  String onlineHostCount(String count) {
    return '$count 台在线';
  }

  @override
  String get taskActions => '任务操作';

  @override
  String get archiveShort => '存档';

  @override
  String get archiveTab => '归档';

  @override
  String get archivedTasks => '已存档';

  @override
  String get delete => '删除';

  @override
  String get deleteTask => '删除会话';

  @override
  String get deleteWarning => '删除后无法继续此会话';

  @override
  String get busyDelete => '请先停止任务，再删除会话';

  @override
  String get stopBeforeDelete => '停止任务';

  @override
  String get deleted => '会话已从预览中删除';

  @override
  String get restored => '已恢复到首页';

  @override
  String get restore => '恢复';

  @override
  String get archiveEmpty => '暂无存档';

  @override
  String get archiveKeepsRunning => '存档不会停止正在执行的任务';

  @override
  String get title => 'Sailry · 移动端预览';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => '移动工作台';

  @override
  String get edition => '移动端预览 / 01';

  @override
  String get intro => '任务、会话与远程工作空间';

  @override
  String get preview => '预览';

  @override
  String get sample => '示例数据 · 操作仅在当前页面生效';

  @override
  String get mixed => '深浅对照';

  @override
  String get dark => '深色';

  @override
  String get light => '浅色';

  @override
  String get gallery => '总览';

  @override
  String get focus => '单屏体验';

  @override
  String get reset => '重置预览';

  @override
  String get page => '选择页面';

  @override
  String get experience => '体验此页';

  @override
  String get backGallery => '返回总览';

  @override
  String get design => '功能与设计';

  @override
  String get footer => 'SAILRY / 移动端';

  @override
  String get footerNote => '本地 HTML 预览 · 无服务连接';

  @override
  String get tasks => '任务';

  @override
  String get chat => '会话';

  @override
  String get hosts => '主机';

  @override
  String get resources => '资源';

  @override
  String get settings => '设置';

  @override
  String get usage => '用量';

  @override
  String get changes => '变更';

  @override
  String get terminal => '终端';

  @override
  String get newTerminal => '新建终端';

  @override
  String get files => '文件';

  @override
  String get project => '项目';

  @override
  String get worktree => '工作树';

  @override
  String get subtitleTasks => '跨主机任务 · 审批与回复优先';

  @override
  String get subtitleChat => '连续对话 · 工具过程按需展开';

  @override
  String get subtitleHosts => '连接状态 · 主机资源与运行进程';

  @override
  String get subtitleResources => '主机 → 项目 → 工作树';

  @override
  String get subtitleChanges => '文件差异 · 暂存与提交';

  @override
  String get subtitleTerminal => '远端终端 · 明确输入控制权';

  @override
  String get subtitleUsage => 'Sailry 会话 · 跨主机聚合';

  @override
  String get subtitleSettings => '本机偏好 · 执行节点配置';

  @override
  String get allHosts => '全部主机';

  @override
  String get connectedHosts => '2 台在线';

  @override
  String get all => '全部';

  @override
  String get running => '运行中';

  @override
  String get waiting => '待处理';

  @override
  String get completed => '已完成';

  @override
  String get taskProgress => '当前任务';

  @override
  String get taskWait => '等待你的决定';

  @override
  String get taskRecent => '最近完成';

  @override
  String get search => '搜索';

  @override
  String get searchTasks => '搜索任务、项目';

  @override
  String get filterTasks => '任务筛选';

  @override
  String get noResults => '没有匹配的任务';

  @override
  String get notification => '通知';

  @override
  String get newTask => '新建任务';

  @override
  String get newConversation => '新会话';

  @override
  String get approveTitle => '更新登录页的布局';

  @override
  String get approveNote => '运行项目测试';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => '整理 API 接口文档';

  @override
  String get questionNote => '等待回复';

  @override
  String get question => '文档使用哪种语言？';

  @override
  String get optionChinese => '中文';

  @override
  String get optionEnglish => '英文';

  @override
  String get reply => '回复';

  @override
  String get approval => '审批';

  @override
  String get viewRequest => '查看请求';

  @override
  String get taskSearch => '优化文件搜索';

  @override
  String get taskSearchNote => '正在检查目录索引';

  @override
  String get taskTest => '修复会话恢复';

  @override
  String get taskTestNote => '正在运行测试';

  @override
  String get taskDone => '补充项目 README';

  @override
  String get taskDoneNote => '3 个文件变更';

  @override
  String get ago => '刚刚';

  @override
  String get minutesAgo => '12 分钟前';

  @override
  String get allow => '允许一次';

  @override
  String get deny => '拒绝';

  @override
  String get approved => '已允许 · 示例';

  @override
  String get denied => '已拒绝 · 示例';

  @override
  String get answered => '已回复 · 示例';

  @override
  String get awaiting => '等待审批';

  @override
  String get working => '正在处理';

  @override
  String get viewChanges => '查看变更';

  @override
  String get viewConversation => '查看会话';

  @override
  String get chatTitle => '更新登录页的布局';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '今天 09:36';

  @override
  String get userMessage => '调整登录页的间距，统一输入框和按钮的样式，保留现有登录逻辑';

  @override
  String get assistantMessage => '我检查了登录页和共享表单组件，已统一输入区域的间距，并补齐键盘焦点样式';

  @override
  String get replyPreview => '任务流程示例';

  @override
  String get phaseThinking => '开始思考';

  @override
  String get phaseReading => '读取文件';

  @override
  String get phaseQuestion => '等待回复';

  @override
  String get phaseEditing => '编辑文件';

  @override
  String get phaseApproval => '等待审批';

  @override
  String get phaseTesting => '运行测试';

  @override
  String get phaseReply => '输出结果';

  @override
  String get phaseFollowup => '处理队列';

  @override
  String get phaseComplete => '全部完成';

  @override
  String get phaseFailed => '测试失败';

  @override
  String get allowShort => '允许';

  @override
  String get queueShort => '队列';

  @override
  String get confirmShort => '确认';

  @override
  String get todoShort => '待办';

  @override
  String get todoInspect => '检查登录页';

  @override
  String get todoEdit => '调整表单样式';

  @override
  String get todoTest => '验证项目测试';

  @override
  String get todoNarrow => '检查窄屏间距';

  @override
  String get workProcess => '处理过程';

  @override
  String workSteps(String count) {
    return '· $count 步';
  }

  @override
  String get questionRecord => '确认布局';

  @override
  String get answerRecorded => '已回复';

  @override
  String get playFlow => '播放任务';

  @override
  String get pauseFlow => '暂停演示';

  @override
  String get nextFlow => '下一步';

  @override
  String get replyingNow => '正在回复';

  @override
  String get toolReadLabel => '读取';

  @override
  String get toolEditLabel => '编辑';

  @override
  String get toolRunLabel => '运行';

  @override
  String get readGroup => '3 个文件';

  @override
  String get readFileResult => '文件已读取';

  @override
  String get readFileProgress => '正在读取文件';

  @override
  String get flowAttachment => '登录页调整：统一表单间距，补齐键盘焦点样式，保留现有登录逻辑';

  @override
  String get readResult => '已读取 Login.tsx 和共享表单样式\n发现移动端按钮宽度与表单不一致';

  @override
  String get layoutFindings => '登录表单沿用了桌面间距，移动端按钮也没有铺满容器';

  @override
  String get layoutQuestion => '移动端登录按钮需要通栏吗';

  @override
  String get questionPending => '等待你的回复';

  @override
  String get wideButton => '使用通栏按钮';

  @override
  String get keepButton => '保持当前宽度';

  @override
  String get editPlan => '我会保留登录逻辑，统一表单间距，并让移动端按钮通栏';

  @override
  String get editPlanKeep => '我会保留按钮宽度和登录逻辑，只调整间距与焦点样式';

  @override
  String get editThinking => '复用现有样式变量，并把布局调整限制在登录表单内';

  @override
  String get editResult => '已更新 3 个文件\n增加焦点样式与移动端布局规则';

  @override
  String get beforeTest => '布局调整已完成，接下来运行项目测试确认没有回归';

  @override
  String get testTool => '运行项目测试';

  @override
  String get testProgress => '执行登录表单测试…\n正在检查焦点与键盘交互';

  @override
  String get testResult => '12 项测试通过\n未发现登录逻辑回归';

  @override
  String get testFailure => '焦点顺序测试未通过\n预期焦点进入密码框，实际停留在用户名输入框';

  @override
  String get testFailed => '测试未通过';

  @override
  String get flowResult => '已统一登录页间距和焦点样式，移动端按钮使用通栏布局。12 项测试通过，登录逻辑保持不变';

  @override
  String get queueSample => '再检查一下窄屏下的按钮间距';

  @override
  String queueCount(String count) {
    return '$count 条排队消息';
  }

  @override
  String queuePaused(String count) {
    return '队列已暂停 · $count';
  }

  @override
  String get pauseQueue => '暂停队列';

  @override
  String get resumeQueue => '继续队列';

  @override
  String get sendNext => '发送下一条';

  @override
  String get enqueue => '加入队列';

  @override
  String get queuedPreview => '已加入示例队列';

  @override
  String get moveUp => '上移';

  @override
  String get followupThinking => '检查现有断点规则，确认窄屏按钮间距是否一致';

  @override
  String get followupTool => '检查窄屏样式';

  @override
  String get followupToolResult => '320px 与 390px 使用相同间距规则';

  @override
  String get followupResult => '窄屏按钮间距一致，无需继续修改';

  @override
  String get deniedResult => '未运行测试，已保留当前修改';

  @override
  String get thinkingNow => '正在思考';

  @override
  String get toolsNow => '正在执行';

  @override
  String get toolPending => '未开始';

  @override
  String get thoughtLive => '先查看登录页和表单组件，确认需要调整的间距和焦点样式';

  @override
  String get toolsShort => '3 项操作';

  @override
  String get thought => '思考过程';

  @override
  String get thoughtContent => '沿用项目已有的表单组件，只调整登录页的布局和焦点样式';

  @override
  String get toolsComplete => '已完成 3 项操作';

  @override
  String get toolRead => '读取登录页与表单组件';

  @override
  String get toolEdit => '更新间距与焦点样式';

  @override
  String get toolDiff => '检查文件差异';

  @override
  String get changedFiles => '3 个文件变更';

  @override
  String get approvalBody => '在 Studio 的 sailry-web 工作树运行测试';

  @override
  String get approvalResolved => '审批已处理';

  @override
  String get chatContinue => '继续描述你的任务';

  @override
  String get describeTask => '描述你的任务';

  @override
  String get send => '发送';

  @override
  String get attach => '添加附件';

  @override
  String get voice => '语音输入';

  @override
  String get voiceNote => '此预览不访问麦克风';

  @override
  String get attachmentNote => '附件示例已添加';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => '移除附件';

  @override
  String get sentPreview => '仅预览，未发送';

  @override
  String get model => '模型';

  @override
  String get modelSource => '当前会话配置 · Studio';

  @override
  String get copy => '复制';

  @override
  String get copied => '已复制';

  @override
  String get copyFailed => '复制不可用，请手动选择文本';

  @override
  String get more => '更多';

  @override
  String get close => '关闭';

  @override
  String get back => '返回';

  @override
  String get cancel => '取消';

  @override
  String get save => '保存';

  @override
  String get select => '选择';

  @override
  String get sessionActions => '会话操作';

  @override
  String get queue => '消息队列';

  @override
  String get queueEmpty => '暂无排队消息';

  @override
  String get fork => '分支会话';

  @override
  String get forked => '已创建示例分支';

  @override
  String get archive => '归档会话';

  @override
  String get archived => '已在预览中归档';

  @override
  String get stop => '停止任务';

  @override
  String get stopped => '任务已在预览中停止';

  @override
  String get stoppedStatus => '已停止';

  @override
  String get hostSubtitle => '你的执行节点';

  @override
  String get pair => '连接主机';

  @override
  String get online => '在线';

  @override
  String get offline => '离线';

  @override
  String get connection => '连接';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 核';

  @override
  String get laptopSystem => '上次在线 2 小时前';

  @override
  String get statusHealthy => '运行正常';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => '内存';

  @override
  String get disk => '磁盘';

  @override
  String get metrics => '资源使用';

  @override
  String get activity => '任务活动';

  @override
  String get lastHour => '最近 60 分钟';

  @override
  String get sessionCount => '会话';

  @override
  String get terminalCount => '终端';

  @override
  String get projectCount => '项目';

  @override
  String get processes => '进程';

  @override
  String get process => '名称';

  @override
  String get network => '网络';

  @override
  String get details => '详情';

  @override
  String get manageHost => '主机详情';

  @override
  String get hostProjects => '主机项目';

  @override
  String get connectionDetails => '连接详情';

  @override
  String get direct => '直连';

  @override
  String get relay => '中继';

  @override
  String get latency => '延迟';

  @override
  String get hostOffline => '主机离线，当前显示上次已知状态';

  @override
  String get retry => '重试';

  @override
  String get retryNote => '预览未连接真实主机';

  @override
  String get pairTitle => '连接一台主机';

  @override
  String get pairDescription => '输入对方主机显示的 6 位配对码';

  @override
  String get pairCode => '配对码';

  @override
  String get pairHint => '配对码有效期 60 秒';

  @override
  String get pairDemo => '模拟连接';

  @override
  String get pairSuccess => '示例主机已添加';

  @override
  String get pairInvalid => '请输入 6 位数字';

  @override
  String get workspace => '工作空间';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => '选择主机';

  @override
  String get selectProject => '选择项目';

  @override
  String get selectBranch => '选择工作树';

  @override
  String get mainBranch => '主工作树';

  @override
  String get featureBranch => '登录页调整';

  @override
  String get connectionTools => '连接';

  @override
  String get workspaceResources => '工作区';

  @override
  String get confirm => '确认';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => '变更、分支与历史';

  @override
  String get gitBranches => '分支';

  @override
  String get gitHistory => '历史';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added 行新增 · $removed 行删除';
  }

  @override
  String get gitActions => 'Git 操作';

  @override
  String get gitFetch => '获取';

  @override
  String get gitPull => '拉取';

  @override
  String get gitPush => '推送';

  @override
  String get gitCurrent => '当前分支';

  @override
  String get gitCreateBranch => '新建分支';

  @override
  String get gitBranchName => '分支名称';

  @override
  String get gitSwitch => '切换分支';

  @override
  String get gitMerge => '合并分支';

  @override
  String get gitDeleteBranch => '删除分支';

  @override
  String get gitHistoryLayout => '调整登录表单间距';

  @override
  String get gitHistoryInit => '初始化登录页';

  @override
  String get gitPreview => '仅模拟 Git 操作，未修改仓库';

  @override
  String get gitDirty => '请先提交当前修改';

  @override
  String get gitSwitchNote => '在当前工作树切换分支；本次仅模拟';

  @override
  String get gitDeleteNote => '删除所选分支；本次仅模拟';

  @override
  String get gitInvalidBranch => '名称不可用或分支已存在';

  @override
  String get review => '评审';

  @override
  String get browseFiles => '浏览工作树';

  @override
  String get reviewFiles => '查看代码变更';

  @override
  String get selectWorkspace => '项目与工作树';

  @override
  String get resourceSummary => '2 个会话 · 1 个终端';

  @override
  String get searchFiles => '搜索文件';

  @override
  String get recentFiles => '文件';

  @override
  String get src => '源代码';

  @override
  String get folder => '文件夹';

  @override
  String get modified => '已修改';

  @override
  String get filePreview => '文件预览';

  @override
  String get fileSample => '此处显示示例文件内容';

  @override
  String get edit => '编辑';

  @override
  String get savePreview => '修改已保存在本次预览';

  @override
  String get unsaved => '未保存';

  @override
  String get discard => '放弃修改';

  @override
  String get discardConfirm => '放弃此文件的未保存修改？';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => '更多资源';

  @override
  String get ssh => 'SSH';

  @override
  String get database => '数据库';

  @override
  String get ports => '端口转发';

  @override
  String get browser => '网页预览';

  @override
  String get portsSub => '1 个转发';

  @override
  String get connectionOwner => '执行节点 · Studio';

  @override
  String get openTerminal => '打开终端';

  @override
  String get tables => '数据表';

  @override
  String get portNote => '示例转发 · 未监听本机端口';

  @override
  String get portTarget => '目标端口';

  @override
  String get localPort => '本机端口';

  @override
  String get closePort => '关闭转发';

  @override
  String get portClosed => '示例转发已关闭';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => '工作区';

  @override
  String get staged => '已暂存';

  @override
  String get diffSummary => '3 个文件';

  @override
  String get stage => '暂存全部';

  @override
  String get unstage => '取消暂存';

  @override
  String get commit => '提交';

  @override
  String get commitTitle => '提交变更';

  @override
  String get commitMessage => '提交说明';

  @override
  String get commitPlaceholder => '描述这次变更';

  @override
  String get commitPreview => '模拟提交';

  @override
  String get committed => '示例提交已完成';

  @override
  String get noChanges => '没有待提交的变更';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => '继续修改';

  @override
  String get diffSelection => '选择变更文件';

  @override
  String get terminalKeyboard => '键盘';

  @override
  String get terminalEnter => '回车';

  @override
  String get terminalOutputLabel => '终端输出';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => '只读观察';

  @override
  String get takeControl => '接管输入';

  @override
  String get hasControl => '输入控制中';

  @override
  String get releaseControl => '释放控制';

  @override
  String get terminalPlaceholder => '输入示例命令';

  @override
  String get terminalPreview => '示例终端 · 不执行命令';

  @override
  String get terminalOutput => '命令已在预览中接收，未执行';

  @override
  String get terminalControlNote => '接管后可发送输入；此处仅模拟控制权';

  @override
  String get usageSubtitle => '仅 Sailry 会话';

  @override
  String get week => '本周';

  @override
  String get month => '本月';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => '响应';

  @override
  String usageCoverage(String priced, String total) {
    return '已计价 $priced / $total 次响应';
  }

  @override
  String get usageEmpty => '暂无用量数据';

  @override
  String get estimatedCost => '估算费用';

  @override
  String get costCoverage => '已计价 42 / 48 次响应';

  @override
  String get partial => '部分数据';

  @override
  String get sourcesPartial => '2 / 3 台主机已更新';

  @override
  String get input => '输入';

  @override
  String get output => '输出';

  @override
  String get cached => '缓存命中';

  @override
  String get modelUsage => '模型分布';

  @override
  String get hostUsage => '主机用量';

  @override
  String get recentRequests => '最近响应';

  @override
  String get allUsage => '用量详情';

  @override
  String get usageNote => '费用为估算，部分响应未计价';

  @override
  String get sourceNote => '离线主机保留上次已知数据';

  @override
  String get profileSubtitle => '移动控制端';

  @override
  String get localSettings => '本机偏好';

  @override
  String get nodeSettings => '执行节点配置';

  @override
  String get appearance => '外观';

  @override
  String get notifications => '通知';

  @override
  String get enabled => '已开启';

  @override
  String get disabled => '已关闭';

  @override
  String get add => '添加';

  @override
  String get configName => '名称';

  @override
  String get configEndpoint => '接口地址';

  @override
  String get configModels => '模型';

  @override
  String get configInstructions => '指令';

  @override
  String get configContent => '内容';

  @override
  String get configEmpty => '暂无条目';

  @override
  String get configDuplicate => '名称已存在';

  @override
  String configDelete(String name) {
    return '删除“$name”？';
  }

  @override
  String get speechInput => '语音输入';

  @override
  String get developerInstructions => '根据任务修改代码并验证结果';

  @override
  String get reviewerInstructions => '检查代码变更并指出问题';

  @override
  String get projectConventions => '项目约定';

  @override
  String get memoryContent => '保持现有代码风格';

  @override
  String get providers => '模型与渠道';

  @override
  String get roles => '分工角色';

  @override
  String get memorySettings => '记忆';

  @override
  String get speech => '语音';

  @override
  String nodeSettingsNote(String host) {
    return '配置保存在 $host';
  }

  @override
  String get about => '关于 Sailry';

  @override
  String get aboutBody => '移动端交互预览，未接入实际服务';

  @override
  String get settingsSaved => '设置已在本次预览中更新';

  @override
  String get modelPicker => '选择模型';

  @override
  String get nodeDefaults => '节点默认配置';

  @override
  String get providerNote => '配置示例 · 凭据由执行节点保存';

  @override
  String get roleNote => '示例角色 · 作用于新建会话';

  @override
  String get auto => '自动';

  @override
  String get manual => '每次询问';

  @override
  String get notificationsNote => '仅控制预览中的通知显示';

  @override
  String get memoryNote => '节点记忆示例';

  @override
  String get speechNote => '使用执行节点的语音配置';

  @override
  String get newTaskHost => '执行主机';

  @override
  String get newTaskProject => '项目';

  @override
  String get newTaskWorktree => '工作树';

  @override
  String get create => '创建';

  @override
  String get taskCreated => '示例会话已创建';

  @override
  String get required => '请先填写任务';

  @override
  String get notificationsEmpty => '没有新通知';

  @override
  String get reviewTitle => '本次设计依据';

  @override
  String get reviewIntro => '核对当前源码后组织页面；以下为预览范围，不代表移动端服务验收';

  @override
  String get reviewConversation => '会话、审批、提问与队列';

  @override
  String get reviewConversationText => '任务聚合入口保留主机、项目和工作树归属；会话内展开工具记录与审批';

  @override
  String get reviewResources => '文件、Git、终端与连接资源';

  @override
  String get reviewResourcesText => '文件编辑、暂存提交和端口保留入口；高频操作优先，详情进入二级页面';

  @override
  String get reviewHosts => '主机连接与监控';

  @override
  String get reviewHostsText => '已配对节点、6 位码连接、资源用量和进程；离线状态不显示为实时在线';

  @override
  String get reviewUsage => '用量与节点配置';

  @override
  String get reviewUsageText => '仅统计 Sailry 会话；跨主机汇总保留不完整状态，费用标明估算及覆盖范围';

  @override
  String get reviewBoundary => '移动端边界';

  @override
  String get reviewBoundaryText =>
      '移动桥接已有连接、会话、终端、用量等接口；本预览仅模拟交互，不启动 Node、模型、配对、终端或插件运行时';

  @override
  String get reviewVisual => '视觉参考';

  @override
  String get reviewVisualText => '参考图 1 的会话层次、图 2 的柔和深浅色卡片与悬浮导航、图 3 的紧凑监控面板';

  @override
  String get hostConnectPrompt => '请连接主机';

  @override
  String get hostDisconnected => '连接已断开';

  @override
  String get language => '语言';

  @override
  String get languageSystem => '跟随系统';

  @override
  String get languageChinese => '简体中文';

  @override
  String get languageEnglish => 'English';

  @override
  String get languageTraditionalChinese => '繁體中文';

  @override
  String get languageJapanese => '日本語';

  @override
  String get languageKorean => '한국어';

  @override
  String get languageFrench => 'Français';

  @override
  String get languageGerman => 'Deutsch';

  @override
  String get languageSpanish => 'Español';

  @override
  String get languagePortugueseBrazil => 'Português (Brasil)';

  @override
  String get languageRussian => 'Русский';

  @override
  String get backgroundConnection => '后台保持连接';

  @override
  String get backgroundConnectionActive => '正在保持主机连接';

  @override
  String get backgroundConnectionFailed => '后台连接未开启，请重试';

  @override
  String get resetReasoning => '重置强度';

  @override
  String get completionAlerts => '完成提示';

  @override
  String get notificationsReadAll => '全部已读';

  @override
  String get notificationsOpen => '查看';

  @override
  String get preferencesFailed => '偏好未保存，请重试';

  @override
  String get connectFirst => '连接主机以开始';

  @override
  String get initializing => '正在启动';

  @override
  String get startupFailed => '启动失败';

  @override
  String get retryConnection => '重试';

  @override
  String get pairAction => '连接';

  @override
  String get pairFailed => '连接失败，请重试';

  @override
  String get pairExpired => '配对码已失效，请获取新码';

  @override
  String get pairing => '正在连接';

  @override
  String get hostUnavailable => '主机未连接';

  @override
  String get hostMetricsFailed => '无法读取主机状态';

  @override
  String get hostProcessesEmpty => '暂无进程';

  @override
  String get hostRegisterProject => '添加项目';

  @override
  String get hostChooseDirectory => '选择目录';

  @override
  String get hostChooseFile => '选择文件';

  @override
  String get hostParentDirectory => '上级目录';

  @override
  String get hostEmptyDirectory => '目录为空';

  @override
  String get hostLoadMore => '加载更多';

  @override
  String get hostProjectName => '项目名称';

  @override
  String get hostProjectPath => '主机上的项目路径';

  @override
  String get hostProjectFailed => '无法添加项目';

  @override
  String get hostUnknown => '暂无数据';

  @override
  String get hostRefresh => '刷新';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => '内存';

  @override
  String get hostMetricDisk => '磁盘';

  @override
  String get failureConflict => '内容已变更，请重新读取后重试';

  @override
  String get failureUnknown => '结果未确认，请先检查主机上的状态';

  @override
  String get failureDenied => '没有操作权限';

  @override
  String get failureUnavailable => '主机未连接';

  @override
  String get failureBusy => '服务正忙，请稍后重试';

  @override
  String get failureGeneric => '操作失败';

  @override
  String get settingsSpeechLanguage => '语言';

  @override
  String get settingsSpeechAuto => '自动识别';

  @override
  String get settingsSpeechChinese => '中文';

  @override
  String get settingsSpeechEnglish => '英语';

  @override
  String get settingsSpeechReady => '语音模型已就绪';

  @override
  String get settingsSpeechDownload => '下载语音模型';

  @override
  String get settingsSpeechFailed => '语音模型未就绪，请重试';

  @override
  String get settingsNoHost => '请先连接主机';

  @override
  String get settingsUnavailable => '暂未提供';

  @override
  String get settingsLoadFailed => '读取失败';

  @override
  String get settingsSaveFailed => '保存失败，草稿已保留';

  @override
  String get settingsConflict => '设置已更新，请重新打开后重试';

  @override
  String get settingsUnknown => '结果未确认，请先刷新查看';

  @override
  String get settingsRetry => '重试';

  @override
  String get settingsLoading => '正在读取';

  @override
  String get settingsRequired => '请填写内容';

  @override
  String get settingsKey => '标识';

  @override
  String get settingsDescription => '说明';

  @override
  String get settingsInstructions => '指令';

  @override
  String get settingsModels => '模型 ID，每行一个';

  @override
  String get settingsApi => 'API 格式';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API 密钥';

  @override
  String get settingsEnabled => '启用';

  @override
  String get settingsDriver => '服务';

  @override
  String get settingsModel => '模型';

  @override
  String get settingsMemoryAuto => '自动记录';

  @override
  String get settingsMemoryBudget => '上下文字节数';

  @override
  String get settingsMemoryReview => '复查间隔（天）';

  @override
  String get settingsMemoryRecords => '记忆条目';

  @override
  String get settingsMemoryKind => '类型';

  @override
  String get settingsMemoryUser => '用户';

  @override
  String get settingsMemoryFeedback => '反馈';

  @override
  String get settingsMemoryProject => '项目';

  @override
  String get settingsMemoryReference => '参考';

  @override
  String get settingsArchived => '已归档';

  @override
  String get settingsEmpty => '暂无记录';

  @override
  String get settingsUsageUnknown => '未知';

  @override
  String get settingsUsagePartial => '部分主机暂不可用';

  @override
  String get settingsUsageCache => '缓存';

  @override
  String get settingsUsageInput => '未缓存输入';

  @override
  String get settingsUsageOutput => '输出';

  @override
  String get settingsUsageDaily => '每日';

  @override
  String get settingsUsageWeekly => '每周';

  @override
  String get settingsUtc => 'UTC';
}

/// The translations for Chinese, using the Han script (`zh_Hant`).
class AppLocalizationsZhHant extends AppLocalizationsZh {
  AppLocalizationsZhHant() : super('zh_Hant');

  @override
  String get updatesVersion => '版本';

  @override
  String get updatesCheck => '檢查更新';

  @override
  String get updatesChecking => '檢查中';

  @override
  String updatesAvailable(String version) {
    return '有新版本 $version';
  }

  @override
  String get updatesDownload => '下載更新';

  @override
  String get updatesCurrent => '已是最新版本';

  @override
  String get updatesUnpublished => '暫無移動端釋出版本';

  @override
  String get updatesCheckFailed => '無法檢查更新';

  @override
  String get updatesOpenFailed => '無法開啟下載連結';

  @override
  String get retryTask => '重試';

  @override
  String get welcomeTitle => '今天，想一起做點什麼？';

  @override
  String get welcomeExplore => '探索專案';

  @override
  String get welcomeExploreDetail => '瞭解結構與關鍵入口';

  @override
  String get welcomeExplorePrompt => '請幫我梳理這個專案的結構，說明關鍵模組和入口。';

  @override
  String get welcomeBuild => '實現想法';

  @override
  String get welcomeBuildDetail => '把想法做出來';

  @override
  String get welcomeBuildPrompt => '我想為這個專案新增一個功能，請先和我確認需求並制定實現方案。';

  @override
  String get welcomeReview => '審查變更';

  @override
  String get welcomeReviewDetail => '檢查改動與潛在問題';

  @override
  String get welcomeReviewPrompt => '請審查當前專案的改動，重點檢查潛在問題和缺失的測試。';

  @override
  String get welcomePlan => '制定計劃';

  @override
  String get welcomePlanDetail => '理清目標與步驟';

  @override
  String get welcomePlanPrompt => '請幫我為接下來的開發工作制定分步驟的計劃。';

  @override
  String get conversationEmpty => '描述你的任務';

  @override
  String get conversationLoading => '正在載入會話';

  @override
  String get conversationReconnecting => '正在重連';

  @override
  String get conversationErrorDetails => '檢視原因';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return '正在重試模型請求 $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return '模型請求已重試 $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count 個工具呼叫';
  }

  @override
  String get conversationGoal => '目標';

  @override
  String get conversationGoalBlocked => '受阻';

  @override
  String conversationGoalBudget(String count) {
    return '預算 $count tokens';
  }

  @override
  String get conversationStepSkipped => '已跳過';

  @override
  String get conversationChild => '子任務';

  @override
  String get conversationChildReadonly => '子任務會話';

  @override
  String get conversationOffline => '連線已斷開';

  @override
  String get conversationUnavailable => '會話不可用';

  @override
  String get conversationFailed => '操作失敗，請重試';

  @override
  String get conversationUnknown => '結果尚未確認，請檢視會話後再操作';

  @override
  String get conversationCheckResult => '確認結果';

  @override
  String get conversationConflict => '配置已更新，請重新開啟後再操作';

  @override
  String get conversationOlder => '載入更早的訊息';

  @override
  String get conversationNew => '新會話';

  @override
  String get conversationNoHost => '請先連線主機';

  @override
  String get conversationNoProject => '請先在主機新增專案';

  @override
  String get conversationNoModel => '請先在主機配置模型';

  @override
  String get conversationNoTasks => '暫無會話';

  @override
  String get conversationNoMessages => '暫無訊息';

  @override
  String get conversationPreviewUnavailable => '訊息暫不可用';

  @override
  String get conversationInterrupted => '已中斷';

  @override
  String get conversationFailedStatus => '處理失敗';

  @override
  String get conversationStopping => '正在停止';

  @override
  String get conversationQueued => '等待處理';

  @override
  String get conversationProcessing => '正在處理';

  @override
  String get conversationUnsynced => '狀態未同步';

  @override
  String get conversationGenerating => '正在回覆';

  @override
  String get conversationWaiting => '等待確認';

  @override
  String get conversationCompacting => '正在壓縮上下文';

  @override
  String get conversationForkConfirm => '從當前記錄建立分支會話？';

  @override
  String get conversationCompacted => '上下文已壓縮';

  @override
  String get conversationToolWaiting => '等待執行';

  @override
  String get conversationToolRunning => '正在執行';

  @override
  String get conversationToolReturned => '已返回';

  @override
  String get conversationToolCancelled => '已取消';

  @override
  String get conversationToolNotExecuted => '未執行';

  @override
  String get conversationToolInterrupted => '已中斷';

  @override
  String get conversationUnsupportedInput => '請在桌面端處理此輸入';

  @override
  String get conversationStartCoding => '開始執行';

  @override
  String get conversationPlanFeedback => '修改建議';

  @override
  String get conversationOther => '其他';

  @override
  String get conversationSubmit => '提交';

  @override
  String get conversationSource => '來源';

  @override
  String get conversationMode => '工作模式';

  @override
  String get conversationCode => '執行';

  @override
  String get conversationPlan => '計劃';

  @override
  String get conversationPermission => '許可權';

  @override
  String get conversationAsk => '每次詢問';

  @override
  String get conversationProject => '專案內';

  @override
  String get conversationFull => '完全訪問';

  @override
  String get conversationReasoning => '思考強度';

  @override
  String get conversationDefault => '預設';

  @override
  String get conversationNone => '關閉';

  @override
  String get conversationMinimal => '最低';

  @override
  String get conversationLow => '低';

  @override
  String get conversationMedium => '中';

  @override
  String get conversationHigh => '高';

  @override
  String get conversationXHigh => '更高';

  @override
  String get conversationMax => '最高';

  @override
  String get conversationBudget => '思考預算';

  @override
  String get conversationAttachment => '附件';

  @override
  String get conversationAttachmentTooLarge => '附件不可讀或超過 64 MB';

  @override
  String get conversationDownload => '檢視附件';

  @override
  String get conversationImageFailed => '圖片無法顯示';

  @override
  String get conversationDownloadFailed => '附件載入失敗';

  @override
  String get conversationReadonly => '此會話已歸檔';

  @override
  String get conversationMicrophoneDenied => '無法訪問麥克風';

  @override
  String get conversationRecordingFailed => '語音識別失敗';

  @override
  String get conversationSpeechDisabled => '語音輸入已關閉';

  @override
  String get conversationSpeechMissing => '請先在設定中下載語音模型';

  @override
  String get conversationRecording => '正在錄音';

  @override
  String get conversationTranscribing => '正在識別';

  @override
  String get conversationRecordReady => '準備錄音';

  @override
  String get conversationStartRecording => '開始錄音';

  @override
  String get conversationFinishRecording => '完成錄音';

  @override
  String get conversationSources => '來源';

  @override
  String get conversationSearchSuggestions => '搜尋建議';

  @override
  String get conversationStats => '會話用量';

  @override
  String get conversationStatsEmpty => '暫無用量';

  @override
  String get conversationStatsOverview => '概覽';

  @override
  String get conversationStatsTokenGroup => 'Token 用量';

  @override
  String get conversationStatsCostGroup => '費用';

  @override
  String get conversationStatsGenerationGroup => '生成';

  @override
  String get conversationStatsTokens => 'Token';

  @override
  String get conversationStatsInput => '輸入';

  @override
  String get conversationStatsOutput => '輸出';

  @override
  String get conversationStatsCached => '快取輸入';

  @override
  String get conversationStatsReasoning => '思考輸出';

  @override
  String get conversationStatsCacheRate => '快取命中';

  @override
  String get conversationStatsCost => '預估費用';

  @override
  String get conversationStatsCostCoverage => '費用覆蓋';

  @override
  String get conversationStatsSpeed => '生成速度';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => '計時覆蓋';

  @override
  String get conversationStatsTurns => '輪次';

  @override
  String get conversationStatsResponses => '模型響應';

  @override
  String get conversationStatsContext => '當前上下文';

  @override
  String get conversationStatsInputCost => '輸入費用';

  @override
  String get conversationStatsOutputCost => '輸出費用';

  @override
  String get conversationStatsCacheReadCost => '快取讀取費用';

  @override
  String get conversationStatsCacheWriteCost => '快取寫入費用';

  @override
  String get messageHistoryUpdated => '會話已更新，原記錄已保留';

  @override
  String get turnUndoUnsaved => '檔案有未儲存的修改，請先儲存或放棄';

  @override
  String get codePlain => '純文字';

  @override
  String get toolArguments => '引數';

  @override
  String get toolResult => '結果';

  @override
  String get toolRaw => '原始結果';

  @override
  String get turnChanges => '本輪變更';

  @override
  String turnChangesCount(String count) {
    return '$count 個檔案';
  }

  @override
  String get turnUndo => '撤銷修改';

  @override
  String get turnUndoAll => '全部撤銷';

  @override
  String get turnUndoConfirm => '撤銷這些檔案在本輪中的修改？後續修改發生衝突時會停止';

  @override
  String get turnUndoDone => '已撤銷';

  @override
  String get turnUndoPartial => '部分修改已撤銷，請檢查剩餘檔案';

  @override
  String get messageActions => '訊息操作';

  @override
  String get messageEdit => '編輯並重新生成';

  @override
  String get messageEditConfirm => '替換此訊息及後續會話？檔案不會隨會話回退';

  @override
  String get messageRewind => '回退至此';

  @override
  String get messageRewindConfirm => '回退至此輪？後續會話會保留為備份，檔案不會回退';

  @override
  String get messageBackup => '檢視會話備份';

  @override
  String get messageRegenerate => '重新生成';

  @override
  String get messageSearch => '搜尋會話';

  @override
  String get messageSearchHint => '搜尋訊息';

  @override
  String get messageSearchMissing => '此訊息已不在當前會話中';

  @override
  String get messageSearchStale => '會話已變更，請重新搜尋';

  @override
  String get messageNoResults => '沒有匹配訊息';

  @override
  String get messageCheck => '確認操作結果';

  @override
  String get messageReference => '引用';

  @override
  String get messageReferenceContext => '此引用屬於該訊息傳送時的上下文';

  @override
  String get toolFailed => '失敗';

  @override
  String toolExitCode(String code) {
    return '退出碼 $code';
  }

  @override
  String toolSignal(String signal) {
    return '訊號 $signal 終止';
  }

  @override
  String get toolTimedOut => '命令超時';

  @override
  String get toolCancelled => '命令已取消';

  @override
  String get toolOutcomeUnknown => '命令結果未知';

  @override
  String get toolQuestionAnswered => '已回答';

  @override
  String get toolQuestionDeclined => '已拒絕';

  @override
  String get toolQuestionCancelled => '已取消';

  @override
  String get fileLinkUnavailable => '無法開啟此連結';

  @override
  String get imagePreview => '圖片預覽';

  @override
  String get fileOpenExternal => '用其他應用開啟';

  @override
  String get fileOpenFailed => '無法開啟檔案';

  @override
  String get fileNoApplication => '沒有可開啟此檔案的應用';

  @override
  String get fileSaveBeforeShare => '儲存修改後分享？';

  @override
  String fileTrashConfirm(String name) {
    return '將“$name”移到主機回收站？未儲存的修改也會丟棄';
  }

  @override
  String get fileTrashUncertain => '刪除結果待確認，請重試查詢';

  @override
  String get fileSaveFailed => '檔案儲存失敗';

  @override
  String get terminalHideKeyboard => '收起鍵盤';

  @override
  String get terminalEscape => 'Esc';

  @override
  String get terminalTab => 'Tab';

  @override
  String get terminalCtrl => 'Ctrl';

  @override
  String get terminalAlt => 'Alt';

  @override
  String get terminalShift => 'Shift';

  @override
  String get terminalCmd => 'Cmd';

  @override
  String get terminalArrowLeft => '向左';

  @override
  String get terminalArrowUp => '向上';

  @override
  String get terminalArrowDown => '向下';

  @override
  String get terminalArrowRight => '向右';

  @override
  String get resourceNoWorkspace => '請選擇已連線主機的工作樹';

  @override
  String get resourceDisconnected => '主機未連線';

  @override
  String get resourceRoot => '根目錄';

  @override
  String get resourceMore => '載入更多';

  @override
  String get resourcePartial => '僅顯示部分內容';

  @override
  String get resourceEmpty => '暫無內容';

  @override
  String get resourceSaveError => '儲存失敗，草稿已保留';

  @override
  String get resourceReloadConfirm => '放棄草稿並讀取最新內容？';

  @override
  String get resourceWorktreeCreate => '新建工作樹';

  @override
  String get resourceSessionServices => '會話服務';

  @override
  String get resourceServicesUnavailable => '服務列表暫不可用';

  @override
  String get resourceNoServices => '未發現服務地址';

  @override
  String get resourceServiceOpen => '開啟服務';

  @override
  String get resourceRemotePort => '遠端埠';

  @override
  String get resourceOpenPort => '轉發埠';

  @override
  String get resourceOpenBrowser => '預覽網頁';

  @override
  String get resourcePreviewFailed => '網頁載入失敗';

  @override
  String get resourcePreviewLink => '無法在預覽中開啟此連結';

  @override
  String get resourceForwardStopped => '轉發已停止';

  @override
  String get resourceTerminalControl => '接管輸入';

  @override
  String get resourceTerminalControlHint => '由另一裝置控制';

  @override
  String get resourceTerminalClaiming => '正在接管';

  @override
  String get resourceTerminalReadOnly => '只讀終端';

  @override
  String get resourceTerminalEnded => '終端已結束';

  @override
  String get resourceTerminalConnecting => '正在連線終端';

  @override
  String get resourceTerminalInput => '終端輸入';

  @override
  String get resourceTerminalPaste => '貼上';

  @override
  String get resourceGitNotRepository => '此目錄不是 Git 倉庫';

  @override
  String get resourceInvalidPort => '請輸入 1–65535 的埠';

  @override
  String get tool_navigate => '開啟頁面';

  @override
  String get tool_back => '返回上一頁';

  @override
  String get tool_forward => '前進一頁';

  @override
  String get tool_refresh => '重新整理頁面';

  @override
  String get tool_right_click => '點選元素';

  @override
  String get tool_clear => '輸入文字';

  @override
  String get tool_select => '選擇選項';

  @override
  String get tool_hover => '懸停元素';

  @override
  String get tool_scroll => '滾動頁面';

  @override
  String get tool_press_key => '按鍵';

  @override
  String get tool_new_tab => '新建標籤頁';

  @override
  String get tool_list_windows => '瀏覽器標籤頁';

  @override
  String get tool_switch_window => '切換標籤頁';

  @override
  String get tool_close_window => '關閉視窗';

  @override
  String get tool_close_session => '關閉瀏覽器';

  @override
  String get tool_screenshot => '擷取頁面';

  @override
  String get tool_print_to_pdf => '匯出 PDF';

  @override
  String get tool_file_upload => '上傳檔案';

  @override
  String get tool_downloads => '檢視下載';

  @override
  String get tool_save_download => '儲存下載檔案';

  @override
  String get tool_evaluate_js => '執行頁面指令碼';

  @override
  String get tool_get_cookies => '讀取 Cookie';

  @override
  String get tool_delete_all_cookies => '修改 Cookie';

  @override
  String get tool_drag_and_drop => '拖動元素';

  @override
  String get tool_focus => '聚焦元素';

  @override
  String get tool_handle_alert => '處理頁面提示';

  @override
  String get tool_database_catalog => '瀏覽資料庫';

  @override
  String get tool_database_query => '查詢資料庫';

  @override
  String get tool_database_execute => '執行資料庫操作';

  @override
  String get tool_search_memory => '檢索記憶';

  @override
  String get tool_review_memories => '整理記憶';

  @override
  String get tool_consolidate_memories => '合併記憶';

  @override
  String get tool_save_memory => '儲存記憶';

  @override
  String get tool_forget_memory => '刪除記憶';

  @override
  String get tool_update_plan => '更新計劃';

  @override
  String get tool_create_goal => '建立目標';

  @override
  String get tool_get_goal => '檢視目標';

  @override
  String get tool_update_goal => '更新目標';

  @override
  String get tool_spawn_agent => '子代理';

  @override
  String get tool_browser_tabs => '瀏覽器標籤頁';

  @override
  String get tool_browser_read => '讀取頁面';

  @override
  String get tool_browser_navigate => '開啟頁面';

  @override
  String get tool_browser_click => '點選元素';

  @override
  String get tool_browser_input => '輸入文字';

  @override
  String get tool_browser_scroll => '滾動頁面';

  @override
  String get tool_browser_back => '返回上一頁';

  @override
  String get tool_browser_forward => '前進一頁';

  @override
  String get tool_browser_refresh => '重新整理頁面';

  @override
  String get tool_browser_open => '新建標籤頁';

  @override
  String get tool_browser_close => '關閉標籤頁';

  @override
  String get tool_browser_focus => '切換標籤頁';

  @override
  String get tool_browser_select => '選擇選項';

  @override
  String get tool_browser_hover => '懸停元素';

  @override
  String get tool_browser_key => '按鍵';

  @override
  String get tool_browser_frame => '切換框架';

  @override
  String get tool_browser_wait => '等待頁面';

  @override
  String get tool_browser_screenshot => '擷取頁面';

  @override
  String get tool_ssh_run => '執行 SSH 命令';

  @override
  String get tool_ssh_transfer => '傳輸 SSH 檔案';

  @override
  String get tool_list_worktrees => '列出工作樹';

  @override
  String get tool_create_worktree => '新建工作樹';

  @override
  String get tool_register_worktree => '新增工作樹';

  @override
  String get tool_remove_worktree => '移除工作樹';

  @override
  String get tool_google_search => '網頁搜尋';

  @override
  String get tool_web_fetch => '網頁抓取';

  @override
  String get tool_fetch_url => '網頁抓取';

  @override
  String get tool_read_file => '讀取檔案';

  @override
  String get tool_write_file => '寫入檔案';

  @override
  String get tool_list_directory => '瀏覽目錄';

  @override
  String get tool_search_files => '搜尋檔案';

  @override
  String get tool_run_command => '執行命令';

  @override
  String get tool_read_command => '檢視後臺命令';

  @override
  String get tool_stop_command => '停止命令';

  @override
  String get tool_load_skill => '載入技能';

  @override
  String get tool_read_skill_resource => '讀取技能資源';

  @override
  String get tool_computer_desktop => '檢視桌面';

  @override
  String get tool_computer_observe => '觀察螢幕';

  @override
  String get tool_computer_input => '操作電腦';

  @override
  String get tool_computer_focus => '切換應用';

  @override
  String get tool_computer_open => '開啟應用';

  @override
  String get tool_git_status => 'Git 狀態';

  @override
  String get tool_git_diff => '檢視差異';

  @override
  String get tool_git_log => 'Git 日誌';

  @override
  String get tool_inspect_image => '識別圖片';

  @override
  String get tool_generate_image => '生成圖片';

  @override
  String get tool_generate_video => '生成影片';

  @override
  String get terminalUnavailable => '終端將在接入後可用';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => '重新整理';

  @override
  String get loading => '正在載入';

  @override
  String get home => '會話';

  @override
  String get idle => '空閒';

  @override
  String get allProjects => '全部專案';

  @override
  String get allWorktrees => '全部工作樹';

  @override
  String get filterProjects => '篩選專案';

  @override
  String get closeSearch => '收起搜尋';

  @override
  String onlineHostCount(String count) {
    return '$count 臺線上';
  }

  @override
  String get taskActions => '任務操作';

  @override
  String get archiveShort => '存檔';

  @override
  String get archiveTab => '歸檔';

  @override
  String get archivedTasks => '已存檔';

  @override
  String get delete => '刪除';

  @override
  String get deleteTask => '刪除會話';

  @override
  String get deleteWarning => '刪除後無法繼續此會話';

  @override
  String get busyDelete => '請先停止任務，再刪除會話';

  @override
  String get stopBeforeDelete => '停止任務';

  @override
  String get deleted => '會話已從預覽中刪除';

  @override
  String get restored => '已恢復到首頁';

  @override
  String get restore => '恢復';

  @override
  String get archiveEmpty => '暫無存檔';

  @override
  String get archiveKeepsRunning => '存檔不會停止正在執行的任務';

  @override
  String get title => 'Sailry · 移動端預覽';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => '移動工作臺';

  @override
  String get edition => '移動端預覽 / 01';

  @override
  String get intro => '任務、會話與遠端工作空間';

  @override
  String get preview => '預覽';

  @override
  String get sample => '示例資料 · 操作僅在當前頁面生效';

  @override
  String get mixed => '深淺對照';

  @override
  String get dark => '深色';

  @override
  String get light => '淺色';

  @override
  String get gallery => '總覽';

  @override
  String get focus => '單屏體驗';

  @override
  String get reset => '重置預覽';

  @override
  String get page => '選擇頁面';

  @override
  String get experience => '體驗此頁';

  @override
  String get backGallery => '返回總覽';

  @override
  String get design => '功能與設計';

  @override
  String get footer => 'SAILRY / 移動端';

  @override
  String get footerNote => '本地 HTML 預覽 · 無服務連線';

  @override
  String get tasks => '任務';

  @override
  String get chat => '會話';

  @override
  String get hosts => '主機';

  @override
  String get resources => '資源';

  @override
  String get settings => '設定';

  @override
  String get usage => '用量';

  @override
  String get changes => '變更';

  @override
  String get terminal => '終端';

  @override
  String get newTerminal => '新建終端';

  @override
  String get files => '檔案';

  @override
  String get project => '專案';

  @override
  String get worktree => '工作樹';

  @override
  String get subtitleTasks => '跨主機任務 · 審批與回覆優先';

  @override
  String get subtitleChat => '連續對話 · 工具過程按需展開';

  @override
  String get subtitleHosts => '連線狀態 · 主機資源與執行程序';

  @override
  String get subtitleResources => '主機 → 專案 → 工作樹';

  @override
  String get subtitleChanges => '檔案差異 · 暫存與提交';

  @override
  String get subtitleTerminal => '遠端終端 · 明確輸入控制權';

  @override
  String get subtitleUsage => 'Sailry 會話 · 跨主機聚合';

  @override
  String get subtitleSettings => '本機偏好 · 執行節點配置';

  @override
  String get allHosts => '全部主機';

  @override
  String get connectedHosts => '2 臺線上';

  @override
  String get all => '全部';

  @override
  String get running => '執行中';

  @override
  String get waiting => '待處理';

  @override
  String get completed => '已完成';

  @override
  String get taskProgress => '當前任務';

  @override
  String get taskWait => '等待你的決定';

  @override
  String get taskRecent => '最近完成';

  @override
  String get search => '搜尋';

  @override
  String get searchTasks => '搜尋任務、專案';

  @override
  String get filterTasks => '任務篩選';

  @override
  String get noResults => '沒有匹配的任務';

  @override
  String get notification => '通知';

  @override
  String get newTask => '新建任務';

  @override
  String get newConversation => '新會話';

  @override
  String get approveTitle => '更新登入頁的佈局';

  @override
  String get approveNote => '執行專案測試';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => '整理 API 介面文件';

  @override
  String get questionNote => '等待回覆';

  @override
  String get question => '文件使用哪種語言？';

  @override
  String get optionChinese => '中文';

  @override
  String get optionEnglish => '英文';

  @override
  String get reply => '回覆';

  @override
  String get approval => '審批';

  @override
  String get viewRequest => '檢視請求';

  @override
  String get taskSearch => '最佳化檔案搜尋';

  @override
  String get taskSearchNote => '正在檢查目錄索引';

  @override
  String get taskTest => '修復會話恢復';

  @override
  String get taskTestNote => '正在執行測試';

  @override
  String get taskDone => '補充專案 README';

  @override
  String get taskDoneNote => '3 個檔案變更';

  @override
  String get ago => '剛剛';

  @override
  String get minutesAgo => '12 分鐘前';

  @override
  String get allow => '允許一次';

  @override
  String get deny => '拒絕';

  @override
  String get approved => '已允許 · 示例';

  @override
  String get denied => '已拒絕 · 示例';

  @override
  String get answered => '已回覆 · 示例';

  @override
  String get awaiting => '等待審批';

  @override
  String get working => '正在處理';

  @override
  String get viewChanges => '檢視變更';

  @override
  String get viewConversation => '檢視會話';

  @override
  String get chatTitle => '更新登入頁的佈局';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '今天 09:36';

  @override
  String get userMessage => '調整登入頁的間距，統一輸入框和按鈕的樣式，保留現有登入邏輯';

  @override
  String get assistantMessage => '我檢查了登入頁和共享表單元件，已統一輸入區域的間距，並補齊鍵盤焦點樣式';

  @override
  String get replyPreview => '任務流程示例';

  @override
  String get phaseThinking => '開始思考';

  @override
  String get phaseReading => '讀取檔案';

  @override
  String get phaseQuestion => '等待回覆';

  @override
  String get phaseEditing => '編輯檔案';

  @override
  String get phaseApproval => '等待審批';

  @override
  String get phaseTesting => '執行測試';

  @override
  String get phaseReply => '輸出結果';

  @override
  String get phaseFollowup => '處理佇列';

  @override
  String get phaseComplete => '全部完成';

  @override
  String get phaseFailed => '測試失敗';

  @override
  String get allowShort => '允許';

  @override
  String get queueShort => '佇列';

  @override
  String get confirmShort => '確認';

  @override
  String get todoShort => '待辦';

  @override
  String get todoInspect => '檢查登入頁';

  @override
  String get todoEdit => '調整表單樣式';

  @override
  String get todoTest => '驗證專案測試';

  @override
  String get todoNarrow => '檢查窄屏間距';

  @override
  String get workProcess => '處理過程';

  @override
  String workSteps(String count) {
    return '· $count 步';
  }

  @override
  String get questionRecord => '確認佈局';

  @override
  String get answerRecorded => '已回覆';

  @override
  String get playFlow => '播放任務';

  @override
  String get pauseFlow => '暫停演示';

  @override
  String get nextFlow => '下一步';

  @override
  String get replyingNow => '正在回覆';

  @override
  String get toolReadLabel => '讀取';

  @override
  String get toolEditLabel => '編輯';

  @override
  String get toolRunLabel => '執行';

  @override
  String get readGroup => '3 個檔案';

  @override
  String get readFileResult => '檔案已讀取';

  @override
  String get readFileProgress => '正在讀取檔案';

  @override
  String get flowAttachment => '登入頁調整：統一表單間距，補齊鍵盤焦點樣式，保留現有登入邏輯';

  @override
  String get readResult => '已讀取 Login.tsx 和共享表單樣式\n發現移動端按鈕寬度與表單不一致';

  @override
  String get layoutFindings => '登入表單沿用了桌面間距，移動端按鈕也沒有鋪滿容器';

  @override
  String get layoutQuestion => '移動端登入按鈕需要通欄嗎';

  @override
  String get questionPending => '等待你的回覆';

  @override
  String get wideButton => '使用通欄按鈕';

  @override
  String get keepButton => '保持當前寬度';

  @override
  String get editPlan => '我會保留登入邏輯，統一表單間距，並讓移動端按鈕通欄';

  @override
  String get editPlanKeep => '我會保留按鈕寬度和登入邏輯，只調整間距與焦點樣式';

  @override
  String get editThinking => '複用現有樣式變數，並把佈局調整限制在登入表單內';

  @override
  String get editResult => '已更新 3 個檔案\n增加焦點樣式與移動端佈局規則';

  @override
  String get beforeTest => '佈局調整已完成，接下來執行專案測試確認沒有迴歸';

  @override
  String get testTool => '執行專案測試';

  @override
  String get testProgress => '執行登入表單測試…\n正在檢查焦點與鍵盤互動';

  @override
  String get testResult => '12 項測試透過\n未發現登入邏輯迴歸';

  @override
  String get testFailure => '焦點順序測試未透過\n預期焦點進入密碼框，實際停留在使用者名稱輸入框';

  @override
  String get testFailed => '測試未透過';

  @override
  String get flowResult => '已統一登入頁間距和焦點樣式，移動端按鈕使用通欄佈局。12 項測試透過，登入邏輯保持不變';

  @override
  String get queueSample => '再檢查一下窄屏下的按鈕間距';

  @override
  String queueCount(String count) {
    return '$count 條排隊訊息';
  }

  @override
  String queuePaused(String count) {
    return '佇列已暫停 · $count';
  }

  @override
  String get pauseQueue => '暫停佇列';

  @override
  String get resumeQueue => '繼續佇列';

  @override
  String get sendNext => '傳送下一條';

  @override
  String get enqueue => '加入佇列';

  @override
  String get queuedPreview => '已加入示例佇列';

  @override
  String get moveUp => '上移';

  @override
  String get followupThinking => '檢查現有斷點規則，確認窄屏按鈕間距是否一致';

  @override
  String get followupTool => '檢查窄屏樣式';

  @override
  String get followupToolResult => '320px 與 390px 使用相同間距規則';

  @override
  String get followupResult => '窄屏按鈕間距一致，無需繼續修改';

  @override
  String get deniedResult => '未執行測試，已保留當前修改';

  @override
  String get thinkingNow => '正在思考';

  @override
  String get toolsNow => '正在執行';

  @override
  String get toolPending => '未開始';

  @override
  String get thoughtLive => '先檢視登入頁和表單元件，確認需要調整的間距和焦點樣式';

  @override
  String get toolsShort => '3 項操作';

  @override
  String get thought => '思考過程';

  @override
  String get thoughtContent => '沿用專案已有的表單元件，只調整登入頁的佈局和焦點樣式';

  @override
  String get toolsComplete => '已完成 3 項操作';

  @override
  String get toolRead => '讀取登入頁與表單元件';

  @override
  String get toolEdit => '更新間距與焦點樣式';

  @override
  String get toolDiff => '檢查檔案差異';

  @override
  String get changedFiles => '3 個檔案變更';

  @override
  String get approvalBody => '在 Studio 的 sailry-web 工作樹執行測試';

  @override
  String get approvalResolved => '審批已處理';

  @override
  String get chatContinue => '繼續描述你的任務';

  @override
  String get describeTask => '描述你的任務';

  @override
  String get send => '傳送';

  @override
  String get attach => '新增附件';

  @override
  String get voice => '語音輸入';

  @override
  String get voiceNote => '此預覽不訪問麥克風';

  @override
  String get attachmentNote => '附件示例已新增';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => '移除附件';

  @override
  String get sentPreview => '僅預覽，未傳送';

  @override
  String get model => '模型';

  @override
  String get modelSource => '當前會話配置 · Studio';

  @override
  String get copy => '複製';

  @override
  String get copied => '已複製';

  @override
  String get copyFailed => '複製不可用，請手動選擇文字';

  @override
  String get more => '更多';

  @override
  String get close => '關閉';

  @override
  String get back => '返回';

  @override
  String get cancel => '取消';

  @override
  String get save => '儲存';

  @override
  String get select => '選擇';

  @override
  String get sessionActions => '會話操作';

  @override
  String get queue => '訊息佇列';

  @override
  String get queueEmpty => '暫無排隊訊息';

  @override
  String get fork => '分支會話';

  @override
  String get forked => '已建立示例分支';

  @override
  String get archive => '歸檔會話';

  @override
  String get archived => '已在預覽中歸檔';

  @override
  String get stop => '停止任務';

  @override
  String get stopped => '任務已在預覽中停止';

  @override
  String get stoppedStatus => '已停止';

  @override
  String get hostSubtitle => '你的執行節點';

  @override
  String get pair => '連線主機';

  @override
  String get online => '線上';

  @override
  String get offline => '離線';

  @override
  String get connection => '連線';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 核';

  @override
  String get laptopSystem => '上次線上 2 小時前';

  @override
  String get statusHealthy => '執行正常';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => '記憶體';

  @override
  String get disk => '磁碟';

  @override
  String get metrics => '資源使用';

  @override
  String get activity => '任務活動';

  @override
  String get lastHour => '最近 60 分鐘';

  @override
  String get sessionCount => '會話';

  @override
  String get terminalCount => '終端';

  @override
  String get projectCount => '專案';

  @override
  String get processes => '程序';

  @override
  String get process => '名稱';

  @override
  String get network => '網路';

  @override
  String get details => '詳情';

  @override
  String get manageHost => '主機詳情';

  @override
  String get hostProjects => '主機專案';

  @override
  String get connectionDetails => '連線詳情';

  @override
  String get direct => '直連';

  @override
  String get relay => '中繼';

  @override
  String get latency => '延遲';

  @override
  String get hostOffline => '主機離線，當前顯示上次已知狀態';

  @override
  String get retry => '重試';

  @override
  String get retryNote => '預覽未連線真實主機';

  @override
  String get pairTitle => '連線一臺主機';

  @override
  String get pairDescription => '輸入對方主機顯示的 6 位配對碼';

  @override
  String get pairCode => '配對碼';

  @override
  String get pairHint => '配對碼有效期 60 秒';

  @override
  String get pairDemo => '模擬連線';

  @override
  String get pairSuccess => '示例主機已新增';

  @override
  String get pairInvalid => '請輸入 6 位數字';

  @override
  String get workspace => '工作空間';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => '選擇主機';

  @override
  String get selectProject => '選擇專案';

  @override
  String get selectBranch => '選擇工作樹';

  @override
  String get mainBranch => '主工作樹';

  @override
  String get featureBranch => '登入頁調整';

  @override
  String get connectionTools => '連線';

  @override
  String get workspaceResources => '工作區';

  @override
  String get confirm => '確認';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => '變更、分支與歷史';

  @override
  String get gitBranches => '分支';

  @override
  String get gitHistory => '歷史';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added 行新增 · $removed 行刪除';
  }

  @override
  String get gitActions => 'Git 操作';

  @override
  String get gitFetch => '獲取';

  @override
  String get gitPull => '拉取';

  @override
  String get gitPush => '推送';

  @override
  String get gitCurrent => '當前分支';

  @override
  String get gitCreateBranch => '新建分支';

  @override
  String get gitBranchName => '分支名稱';

  @override
  String get gitSwitch => '切換分支';

  @override
  String get gitMerge => '合併分支';

  @override
  String get gitDeleteBranch => '刪除分支';

  @override
  String get gitHistoryLayout => '調整登入表單間距';

  @override
  String get gitHistoryInit => '初始化登入頁';

  @override
  String get gitPreview => '僅模擬 Git 操作，未修改倉庫';

  @override
  String get gitDirty => '請先提交當前修改';

  @override
  String get gitSwitchNote => '在當前工作樹切換分支；本次僅模擬';

  @override
  String get gitDeleteNote => '刪除所選分支；本次僅模擬';

  @override
  String get gitInvalidBranch => '名稱不可用或分支已存在';

  @override
  String get review => '評審';

  @override
  String get browseFiles => '瀏覽工作樹';

  @override
  String get reviewFiles => '檢視程式碼變更';

  @override
  String get selectWorkspace => '專案與工作樹';

  @override
  String get resourceSummary => '2 個會話 · 1 個終端';

  @override
  String get searchFiles => '搜尋檔案';

  @override
  String get recentFiles => '檔案';

  @override
  String get src => '原始碼';

  @override
  String get folder => '資料夾';

  @override
  String get modified => '已修改';

  @override
  String get filePreview => '檔案預覽';

  @override
  String get fileSample => '此處顯示示例檔案內容';

  @override
  String get edit => '編輯';

  @override
  String get savePreview => '修改已儲存在本次預覽';

  @override
  String get unsaved => '未儲存';

  @override
  String get discard => '放棄修改';

  @override
  String get discardConfirm => '放棄此檔案的未儲存修改？';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => '更多資源';

  @override
  String get ssh => 'SSH';

  @override
  String get database => '資料庫';

  @override
  String get ports => '埠轉發';

  @override
  String get browser => '網頁預覽';

  @override
  String get portsSub => '1 個轉發';

  @override
  String get connectionOwner => '執行節點 · Studio';

  @override
  String get openTerminal => '開啟終端';

  @override
  String get tables => '資料表';

  @override
  String get portNote => '示例轉發 · 未監聽本機埠';

  @override
  String get portTarget => '目標埠';

  @override
  String get localPort => '本機埠';

  @override
  String get closePort => '關閉轉發';

  @override
  String get portClosed => '示例轉發已關閉';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => '工作區';

  @override
  String get staged => '已暫存';

  @override
  String get diffSummary => '3 個檔案';

  @override
  String get stage => '暫存全部';

  @override
  String get unstage => '取消暫存';

  @override
  String get commit => '提交';

  @override
  String get commitTitle => '提交變更';

  @override
  String get commitMessage => '提交說明';

  @override
  String get commitPlaceholder => '描述這次變更';

  @override
  String get commitPreview => '模擬提交';

  @override
  String get committed => '示例提交已完成';

  @override
  String get noChanges => '沒有待提交的變更';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => '繼續修改';

  @override
  String get diffSelection => '選擇變更檔案';

  @override
  String get terminalKeyboard => '鍵盤';

  @override
  String get terminalEnter => '回車';

  @override
  String get terminalOutputLabel => '終端輸出';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => '只讀觀察';

  @override
  String get takeControl => '接管輸入';

  @override
  String get hasControl => '輸入控制中';

  @override
  String get releaseControl => '釋放控制';

  @override
  String get terminalPlaceholder => '輸入示例命令';

  @override
  String get terminalPreview => '示例終端 · 不執行命令';

  @override
  String get terminalOutput => '命令已在預覽中接收，未執行';

  @override
  String get terminalControlNote => '接管後可傳送輸入；此處僅模擬控制權';

  @override
  String get usageSubtitle => '僅 Sailry 會話';

  @override
  String get week => '本週';

  @override
  String get month => '本月';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => '響應';

  @override
  String usageCoverage(String priced, String total) {
    return '已計價 $priced / $total 次響應';
  }

  @override
  String get usageEmpty => '暫無用量資料';

  @override
  String get estimatedCost => '估算費用';

  @override
  String get costCoverage => '已計價 42 / 48 次響應';

  @override
  String get partial => '部分資料';

  @override
  String get sourcesPartial => '2 / 3 臺主機已更新';

  @override
  String get input => '輸入';

  @override
  String get output => '輸出';

  @override
  String get cached => '快取命中';

  @override
  String get modelUsage => '模型分佈';

  @override
  String get hostUsage => '主機用量';

  @override
  String get recentRequests => '最近響應';

  @override
  String get allUsage => '用量詳情';

  @override
  String get usageNote => '費用為估算，部分響應未計價';

  @override
  String get sourceNote => '離線主機保留上次已知資料';

  @override
  String get profileSubtitle => '移動控制端';

  @override
  String get localSettings => '本機偏好';

  @override
  String get nodeSettings => '執行節點配置';

  @override
  String get appearance => '外觀';

  @override
  String get notifications => '通知';

  @override
  String get enabled => '已開啟';

  @override
  String get disabled => '已關閉';

  @override
  String get add => '新增';

  @override
  String get configName => '名稱';

  @override
  String get configEndpoint => '介面地址';

  @override
  String get configModels => '模型';

  @override
  String get configInstructions => '指令';

  @override
  String get configContent => '內容';

  @override
  String get configEmpty => '暫無條目';

  @override
  String get configDuplicate => '名稱已存在';

  @override
  String configDelete(String name) {
    return '刪除“$name”？';
  }

  @override
  String get speechInput => '語音輸入';

  @override
  String get developerInstructions => '根據任務修改程式碼並驗證結果';

  @override
  String get reviewerInstructions => '檢查程式碼變更並指出問題';

  @override
  String get projectConventions => '專案約定';

  @override
  String get memoryContent => '保持現有程式碼風格';

  @override
  String get providers => '模型與渠道';

  @override
  String get roles => '分工角色';

  @override
  String get memorySettings => '記憶';

  @override
  String get speech => '語音';

  @override
  String nodeSettingsNote(String host) {
    return '配置儲存在 $host';
  }

  @override
  String get about => '關於 Sailry';

  @override
  String get aboutBody => '移動端互動預覽，未接入實際服務';

  @override
  String get settingsSaved => '設定已在本次預覽中更新';

  @override
  String get modelPicker => '選擇模型';

  @override
  String get nodeDefaults => '節點預設配置';

  @override
  String get providerNote => '配置示例 · 憑據由執行節點儲存';

  @override
  String get roleNote => '示例角色 · 作用於新建會話';

  @override
  String get auto => '自動';

  @override
  String get manual => '每次詢問';

  @override
  String get notificationsNote => '僅控制預覽中的通知顯示';

  @override
  String get memoryNote => '節點記憶示例';

  @override
  String get speechNote => '使用執行節點的語音配置';

  @override
  String get newTaskHost => '執行主機';

  @override
  String get newTaskProject => '專案';

  @override
  String get newTaskWorktree => '工作樹';

  @override
  String get create => '建立';

  @override
  String get taskCreated => '示例會話已建立';

  @override
  String get required => '請先填寫任務';

  @override
  String get notificationsEmpty => '沒有新通知';

  @override
  String get reviewTitle => '本次設計依據';

  @override
  String get reviewIntro => '核對當前原始碼後組織頁面；以下為預覽範圍，不代表移動端服務驗收';

  @override
  String get reviewConversation => '會話、審批、提問與佇列';

  @override
  String get reviewConversationText => '任務聚合入口保留主機、專案和工作樹歸屬；會話內展開工具記錄與審批';

  @override
  String get reviewResources => '檔案、Git、終端與連線資源';

  @override
  String get reviewResourcesText => '檔案編輯、暫存提交和埠保留入口；高頻操作優先，詳情進入二級頁面';

  @override
  String get reviewHosts => '主機連線與監控';

  @override
  String get reviewHostsText => '已配對節點、6 位碼連線、資源用量和程序；離線狀態不顯示為實時線上';

  @override
  String get reviewUsage => '用量與節點配置';

  @override
  String get reviewUsageText => '僅統計 Sailry 會話；跨主機彙總保留不完整狀態，費用標明估算及覆蓋範圍';

  @override
  String get reviewBoundary => '移動端邊界';

  @override
  String get reviewBoundaryText =>
      '移動橋接已有連線、會話、終端、用量等介面；本預覽僅模擬互動，不啟動 Node、模型、配對、終端或外掛執行時';

  @override
  String get reviewVisual => '視覺參考';

  @override
  String get reviewVisualText => '參考圖 1 的會話層次、圖 2 的柔和深淺色卡片與懸浮導航、圖 3 的緊湊監控面板';

  @override
  String get hostConnectPrompt => '請連線主機';

  @override
  String get hostDisconnected => '連線已斷開';

  @override
  String get language => '語言';

  @override
  String get languageSystem => '跟隨系統';

  @override
  String get languageChinese => '简体中文';

  @override
  String get languageEnglish => 'English';

  @override
  String get languageTraditionalChinese => '繁體中文';

  @override
  String get languageJapanese => '日本語';

  @override
  String get languageKorean => '한국어';

  @override
  String get languageFrench => 'Français';

  @override
  String get languageGerman => 'Deutsch';

  @override
  String get languageSpanish => 'Español';

  @override
  String get languagePortugueseBrazil => 'Português (Brasil)';

  @override
  String get languageRussian => 'Русский';

  @override
  String get backgroundConnection => '後臺保持連線';

  @override
  String get backgroundConnectionActive => '正在保持主機連線';

  @override
  String get backgroundConnectionFailed => '後臺連線未開啟，請重試';

  @override
  String get resetReasoning => '重置強度';

  @override
  String get completionAlerts => '完成提示';

  @override
  String get notificationsReadAll => '全部已讀';

  @override
  String get notificationsOpen => '檢視';

  @override
  String get preferencesFailed => '偏好未儲存，請重試';

  @override
  String get connectFirst => '連線主機以開始';

  @override
  String get initializing => '正在啟動';

  @override
  String get startupFailed => '啟動失敗';

  @override
  String get retryConnection => '重試';

  @override
  String get pairAction => '連線';

  @override
  String get pairFailed => '連線失敗，請重試';

  @override
  String get pairExpired => '配對碼已失效，請獲取新碼';

  @override
  String get pairing => '正在連線';

  @override
  String get hostUnavailable => '主機未連線';

  @override
  String get hostMetricsFailed => '無法讀取主機狀態';

  @override
  String get hostProcessesEmpty => '暫無程序';

  @override
  String get hostRegisterProject => '新增專案';

  @override
  String get hostChooseDirectory => '選擇目錄';

  @override
  String get hostChooseFile => '選擇檔案';

  @override
  String get hostParentDirectory => '上級目錄';

  @override
  String get hostEmptyDirectory => '目錄為空';

  @override
  String get hostLoadMore => '載入更多';

  @override
  String get hostProjectName => '專案名稱';

  @override
  String get hostProjectPath => '主機上的專案路徑';

  @override
  String get hostProjectFailed => '無法新增專案';

  @override
  String get hostUnknown => '暫無資料';

  @override
  String get hostRefresh => '重新整理';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => '記憶體';

  @override
  String get hostMetricDisk => '磁碟';

  @override
  String get failureConflict => '內容已變更，請重新讀取後重試';

  @override
  String get failureUnknown => '結果未確認，請先檢查主機上的狀態';

  @override
  String get failureDenied => '沒有操作許可權';

  @override
  String get failureUnavailable => '主機未連線';

  @override
  String get failureBusy => '服務正忙，請稍後重試';

  @override
  String get failureGeneric => '操作失敗';

  @override
  String get settingsSpeechLanguage => '語言';

  @override
  String get settingsSpeechAuto => '自動識別';

  @override
  String get settingsSpeechChinese => '中文';

  @override
  String get settingsSpeechEnglish => '英語';

  @override
  String get settingsSpeechReady => '語音模型已就緒';

  @override
  String get settingsSpeechDownload => '下載語音模型';

  @override
  String get settingsSpeechFailed => '語音模型未就緒，請重試';

  @override
  String get settingsNoHost => '請先連線主機';

  @override
  String get settingsUnavailable => '暫未提供';

  @override
  String get settingsLoadFailed => '讀取失敗';

  @override
  String get settingsSaveFailed => '儲存失敗，草稿已保留';

  @override
  String get settingsConflict => '設定已更新，請重新開啟後重試';

  @override
  String get settingsUnknown => '結果未確認，請先重新整理檢視';

  @override
  String get settingsRetry => '重試';

  @override
  String get settingsLoading => '正在讀取';

  @override
  String get settingsRequired => '請填寫內容';

  @override
  String get settingsKey => '標識';

  @override
  String get settingsDescription => '說明';

  @override
  String get settingsInstructions => '指令';

  @override
  String get settingsModels => '模型 ID，每行一個';

  @override
  String get settingsApi => 'API 格式';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API 金鑰';

  @override
  String get settingsEnabled => '啟用';

  @override
  String get settingsDriver => '服務';

  @override
  String get settingsModel => '模型';

  @override
  String get settingsMemoryAuto => '自動記錄';

  @override
  String get settingsMemoryBudget => '上下文位元組數';

  @override
  String get settingsMemoryReview => '複查間隔（天）';

  @override
  String get settingsMemoryRecords => '記憶條目';

  @override
  String get settingsMemoryKind => '型別';

  @override
  String get settingsMemoryUser => '使用者';

  @override
  String get settingsMemoryFeedback => '反饋';

  @override
  String get settingsMemoryProject => '專案';

  @override
  String get settingsMemoryReference => '參考';

  @override
  String get settingsArchived => '已歸檔';

  @override
  String get settingsEmpty => '暫無記錄';

  @override
  String get settingsUsageUnknown => '未知';

  @override
  String get settingsUsagePartial => '部分主機暫不可用';

  @override
  String get settingsUsageCache => '快取';

  @override
  String get settingsUsageInput => '未快取輸入';

  @override
  String get settingsUsageOutput => '輸出';

  @override
  String get settingsUsageDaily => '每日';

  @override
  String get settingsUsageWeekly => '每週';

  @override
  String get settingsUtc => 'UTC';
}
