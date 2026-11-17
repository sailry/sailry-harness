// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Chinese (`zh`).
class AppLocalizationsZh extends AppLocalizations {
  AppLocalizationsZh([String locale = 'zh']) : super(locale);

  @override
  String get guidanceWithdraw => '撤回引导';

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
  String get conversationStepSkipped => '已跳过';

  @override
  String get conversationChild => '子任务';

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
  String get conversationContinued => '已接续';

  @override
  String get conversationFailedStatus => '处理失败';

  @override
  String get conversationStopping => '正在停止';

  @override
  String get conversationQueued => '等待处理';

  @override
  String get conversationGenerating => '正在回复';

  @override
  String get conversationWaiting => '等待确认';

  @override
  String get conversationWaitingChild => '等待 1 个子代理';

  @override
  String conversationWaitingChildren(String count) {
    return '等待 $count 个子代理';
  }

  @override
  String get conversationCompacting => '正在压缩上下文';

  @override
  String get conversationWaitingController => '等待控制端更新登录';

  @override
  String get conversationCompacted => '上下文已压缩';

  @override
  String get conversationToolWaiting => '等待执行';

  @override
  String get conversationStartCoding => '开始执行';

  @override
  String get conversationPlanFeedback => '修改建议';

  @override
  String get conversationOther => '其他';

  @override
  String get conversationSubmit => '提交';

  @override
  String get conversationMode => '工作模式';

  @override
  String get conversationCode => '标准';

  @override
  String get conversationPlan => '计划';

  @override
  String get conversationFlagship => '协作';

  @override
  String get conversationAsk => '每次询问';

  @override
  String get conversationProject => '项目内';

  @override
  String get conversationFull => '完全访问';

  @override
  String get conversationPermission => '权限';

  @override
  String get conversationAskDescription => '修改文件和执行命令前询问';

  @override
  String get conversationProjectDescription => '自动修改项目文件，执行命令前询问';

  @override
  String get conversationFullDescription => '自动修改文件和执行命令，可影响项目外文件';

  @override
  String get conversationBudget => '思考预算';

  @override
  String get conversationAttachment => '附件';

  @override
  String get conversationImageFailed => '图片无法显示';

  @override
  String get conversationImageUnavailable => '图片不可用';

  @override
  String get conversationDownloadFailed => '附件加载失败';

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
  String get turnChanges => '本轮变更';

  @override
  String turnChangesCount(String count) {
    return '$count 个文件';
  }

  @override
  String get fileChangesApprovalOne => '将补丁应用到 1 个文件';

  @override
  String fileChangesApproval(String count) {
    return '将补丁应用到 $count 个文件';
  }

  @override
  String get fileChangesAdded => '新增';

  @override
  String get fileChangesModified => '修改';

  @override
  String get fileChangesRenamed => '重命名';

  @override
  String get fileChangesDeleted => '删除';

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
  String get messageRewindConfirm => '保留此回合并移除后续消息？文件不变';

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
  String get resourceTerminalEnded => '终端已结束';

  @override
  String get resourceTerminalConnecting => '正在连接终端';

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
  String get unassignedChats => '对话';

  @override
  String get allWorktrees => '全部工作树';

  @override
  String get tasksActive => '进行中';

  @override
  String get tasksToday => '今天';

  @override
  String get yesterday => '昨天';

  @override
  String get tasksThisWeek => '本周';

  @override
  String get tasksEarlier => '更早';

  @override
  String get filterProjects => '筛选项目';

  @override
  String get archiveShort => '归档';

  @override
  String get archiveTab => '归档';

  @override
  String get delete => '删除';

  @override
  String get deleteTask => '删除会话';

  @override
  String get deleteWarning => '删除后无法继续此会话';

  @override
  String get restored => '已恢复到首页';

  @override
  String get restore => '恢复';

  @override
  String get archiveEmpty => '暂无归档';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => '移动工作台';

  @override
  String get preview => '预览';

  @override
  String get mixed => '深浅对照';

  @override
  String get dark => '深色';

  @override
  String get light => '浅色';

  @override
  String get brandColor => '主题色';

  @override
  String get brandOrange => '橙色';

  @override
  String get brandBlue => '蓝色';

  @override
  String get brandGreen => '绿色';

  @override
  String get brandPurple => '紫色';

  @override
  String get brandPink => '粉色';

  @override
  String get brandTeal => '青色';

  @override
  String get focus => '单屏体验';

  @override
  String get page => '选择页面';

  @override
  String get design => '功能与设计';

  @override
  String get footer => 'SAILRY / 移动端';

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
  String get allHosts => '全部主机';

  @override
  String get all => '全部';

  @override
  String get running => '运行中';

  @override
  String get waiting => '待处理';

  @override
  String get completed => '已完成';

  @override
  String get search => '搜索';

  @override
  String get searchTasks => '搜索任务、项目';

  @override
  String get noResults => '没有匹配的任务';

  @override
  String get notification => '通知';

  @override
  String get newConversation => '新会话';

  @override
  String get question => '文档使用哪种语言？';

  @override
  String get reply => '回复';

  @override
  String get approval => '审批';

  @override
  String get allow => '允许一次';

  @override
  String get deny => '拒绝';

  @override
  String get working => '正在处理';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '今天 09:36';

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
  String get questionPending => '等待你的回复';

  @override
  String queueCount(String count) {
    return '$count 条排队消息';
  }

  @override
  String queuePaused(String count) {
    return '队列已暂停 · $count';
  }

  @override
  String queueGuidance(String queue) {
    return '$queue，引导等待加入';
  }

  @override
  String get pauseQueue => '暂停队列';

  @override
  String get resumeQueue => '继续队列';

  @override
  String get moveUp => '上移';

  @override
  String get thinkingNow => '正在思考';

  @override
  String get thought => '思考过程';

  @override
  String get describeTask => '描述你的任务';

  @override
  String get send => '发送';

  @override
  String get attach => '添加附件';

  @override
  String get voice => '语音输入';

  @override
  String get attachment => '附件';

  @override
  String get removeAttachment => '移除附件';

  @override
  String get model => '模型';

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
  String get queue => '消息队列';

  @override
  String get queueEmpty => '暂无排队消息';

  @override
  String get fork => '分支会话';

  @override
  String get stop => '停止任务';

  @override
  String get stoppedStatus => '已停止';

  @override
  String get pair => '连接主机';

  @override
  String get online => '在线';

  @override
  String get offline => '离线';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => '内存';

  @override
  String get metrics => '资源使用';

  @override
  String get activity => '任务活动';

  @override
  String get processes => '进程';

  @override
  String get network => '网络';

  @override
  String get relay => '中继';

  @override
  String get retry => '重试';

  @override
  String get pairTitle => '连接一台主机';

  @override
  String get pairCode => '配对码';

  @override
  String get pairInvalid => '请输入 6 位数字';

  @override
  String get workspace => '工作空间';

  @override
  String get selectHost => '选择主机';

  @override
  String get confirm => '确认';

  @override
  String get git => 'Git';

  @override
  String get gitBranches => '分支';

  @override
  String get gitHistory => '历史';

  @override
  String gitViewAll(String count) {
    return '查看全部 $count 个变更';
  }

  @override
  String terminalRunningCount(String count) {
    return '$count 个运行中';
  }

  @override
  String get gitUnstaged => '未暂存';

  @override
  String get gitStageFile => '暂存';

  @override
  String gitCommitCount(String count) {
    return '提交 $count 个';
  }

  @override
  String get gitRemote => '远程';

  @override
  String get gitCurrentShort => '当前';

  @override
  String gitDiscardConfirm(String name) {
    return '放弃“$name”的全部修改？';
  }

  @override
  String gitTrashConfirm(String name) {
    return '将“$name”移到主机回收站？';
  }

  @override
  String get gitCommitOptions => '提交选项';

  @override
  String get gitSkipHooks => '跳过钩子';

  @override
  String get gitPushAfterCommit => '提交后推送';

  @override
  String get fileShare => '分享';

  @override
  String get fileTrash => '移到回收站';

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
  String get review => '评审';

  @override
  String get selectWorkspace => '项目与工作树';

  @override
  String get searchFiles => '搜索文件';

  @override
  String get src => '源代码';

  @override
  String get folder => '文件夹';

  @override
  String get modified => '已修改';

  @override
  String get edit => '编辑';

  @override
  String get unsaved => '未保存';

  @override
  String get discard => '放弃修改';

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
  String get openFile => '打开文件';

  @override
  String get closePort => '关闭转发';

  @override
  String get workingTree => '工作区';

  @override
  String get staged => '已暂存';

  @override
  String get stage => '暂存全部';

  @override
  String get unstage => '取消暂存';

  @override
  String get commit => '提交';

  @override
  String get commitMessage => '提交说明';

  @override
  String get noChanges => '没有待提交的变更';

  @override
  String get terminalKeyboard => '键盘';

  @override
  String get terminalEnter => '回车';

  @override
  String get readOnly => '只读观察';

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
  String get estimatedCost => '估算费用（USD）';

  @override
  String get partial => '部分数据';

  @override
  String get input => '输入';

  @override
  String get output => '输出';

  @override
  String get cached => '缓存命中';

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
  String get providers => '模型与渠道';

  @override
  String get speech => '语音';

  @override
  String get about => '关于 Sailry';

  @override
  String get modelPicker => '选择模型';

  @override
  String get team => '团队';

  @override
  String get executionModel => '单模型';

  @override
  String get teamDefault => '默认团队';

  @override
  String get teamLead => '负责人';

  @override
  String get memberReset => '重置模型';

  @override
  String get auto => '自动';

  @override
  String get manual => '每次询问';

  @override
  String get required => '请先填写任务';

  @override
  String get hostConnectPrompt => '请连接主机';

  @override
  String get hostDisconnected => '主机暂时离线';

  @override
  String get hostUpdateRequired => '请更新主机后再连接';

  @override
  String get appUpdateRequired => '请更新此应用后再连接';

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
  String get preferencesFailed => '偏好未保存，请重试';

  @override
  String get startupFailed => '启动失败';

  @override
  String get pairAction => '连接';

  @override
  String get pairFailed => '连接失败，请重试';

  @override
  String get pairExpired => '配对码已失效，请获取新码';

  @override
  String get pairing => '正在连接';

  @override
  String get hostRemove => '移除主机';

  @override
  String hostRemoveConfirm(String name) {
    return '移除“$name”并撤销此设备的访问？主机数据保留；若这是最后一台设备，需要管理员恢复访问。';
  }

  @override
  String get hostRemoved => '已移除主机';

  @override
  String get hostOriginDirect => '直接配对';

  @override
  String hostOriginSource(String name) {
    return '来自 $name';
  }

  @override
  String get hostRemoveFailed => '无法移除主机';

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
  String get settingsLoadFailed => '读取失败';

  @override
  String get settingsSaveFailed => '保存失败，草稿已保留';

  @override
  String get settingsConflict => '设置已更新，请重新打开后重试';

  @override
  String get settingsUnknown => '结果未确认，请先刷新查看';

  @override
  String get settingsRequired => '请填写内容';

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
  String get apiChatCompletions => 'Chat Completions';

  @override
  String get apiResponses => 'Responses';

  @override
  String get apiAnthropic => 'Anthropic';

  @override
  String get apiGemini => 'Gemini';

  @override
  String get apiDeepSeek => 'DeepSeek';

  @override
  String get apiAzureOpenAi => 'Azure OpenAI';

  @override
  String get apiAzureAi => 'Azure AI';

  @override
  String get apiPlugin => '插件';

  @override
  String get providerNew => '新建服务';

  @override
  String get providerEmpty => '暂无模型服务';

  @override
  String get providerDesktopHint => 'OAuth 模型使用桌面端操作';

  @override
  String get providerConnection => '连接';

  @override
  String get providerKeyUnset => '未设置';

  @override
  String get providerShowKey => '显示密钥';

  @override
  String get providerHideKey => '隐藏密钥';

  @override
  String providerModelCount(String count) {
    return '$count 个模型';
  }

  @override
  String get providerFetch => '获取';

  @override
  String get providerFetchTitle => '获取模型';

  @override
  String providerFetched(String count) {
    return '接口返回 $count 个';
  }

  @override
  String get providerSelectAll => '全选';

  @override
  String get providerSelectNone => '取消全选';

  @override
  String get providerSearch => '搜索模型';

  @override
  String get providerAdded => '已添加';

  @override
  String providerContext(String tokens) {
    return '$tokens 上下文';
  }

  @override
  String providerAddCount(String count) {
    return '添加 $count 个';
  }

  @override
  String get providerModelId => '模型 ID';

  @override
  String get providerModelExists => '模型已存在';

  @override
  String get providerDefault => '默认';

  @override
  String get providerBasic => '基本';

  @override
  String get providerContextLength => '上下文长度';

  @override
  String get providerCapabilities => '能力';

  @override
  String get providerVision => '图片';

  @override
  String get providerTools => '工具';

  @override
  String get providerReasoning => '推理';

  @override
  String get providerCapUnderstands => '理解';

  @override
  String get providerCapCan => '支持';

  @override
  String get providerCapGenerates => '生成';

  @override
  String get providerGenerateImage => '图片';

  @override
  String get providerGenerateVideo => '视频';

  @override
  String get providerGenerateSpeech => '语音';

  @override
  String get providerDeploymentId => '部署名';

  @override
  String get providerHeaders => '自定义请求头';

  @override
  String get providerHeadersHint => '每行一个，格式为 名称: 值';

  @override
  String get providerHeadersInvalid => '每行请求头都需要名称和冒号';

  @override
  String get providerEfforts => '思考强度';

  @override
  String get providerEffortPlaceholder => '强度或 Token 预算';

  @override
  String get providerEffortInvalid => '思考强度无效，请检查名称、重复项和接口支持';

  @override
  String get providerEffortDefault => '默认强度';

  @override
  String get providerEffortSetDefault => '设为默认';

  @override
  String get providerSetDefault => '设为默认模型';

  @override
  String get providerIsDefault => '默认模型';

  @override
  String get providerDeleteModel => '删除模型';

  @override
  String get providerDelete => '删除服务';

  @override
  String get providerNumberInvalid => '请输入正整数';

  @override
  String get providerLimitsInvalid => '最大输出不能超过上下文长度';

  @override
  String get settingsUsageUnknown => '未知';

  @override
  String get settingsUsagePartial => '部分主机数据未更新';

  @override
  String get settingsUsageCache => '缓存';

  @override
  String get settingsUsageInput => '未缓存';

  @override
  String get settingsUsageOutput => '输出';

  @override
  String get usageRange24h => '24 小时';

  @override
  String get usageRange7d => '7 天';

  @override
  String get usageRange30d => '30 天';

  @override
  String get usageRange90d => '90 天';

  @override
  String get usageRange1y => '1 年';

  @override
  String get usageReasoning => '推理';

  @override
  String get usageHitRate => '缓存命中率';

  @override
  String get usageSpeed => '平均速度';

  @override
  String get usageSpeedUnit => 'tok/s';

  @override
  String get usageCostPartial => '部分';

  @override
  String get usageTrend => 'Token 趋势';

  @override
  String get usageLocalTime => '按本地时间';

  @override
  String usageActiveDays(String count) {
    return '$count 个活跃日';
  }

  @override
  String get usageBreakdown => '分布';

  @override
  String get usageProvider => '供应商';

  @override
  String get usageShowAll => '显示全部';

  @override
  String get usageCollapse => '收起';

  @override
  String get usageUnassigned => '未归类';

  @override
  String usageHit(String value) {
    return '命中 $value';
  }

  @override
  String usageResponseCount(String count) {
    return '$count 响应';
  }

  @override
  String get artifactContents => '目录';

  @override
  String get artifactZoomIn => '放大';

  @override
  String get artifactZoomOut => '缩小';

  @override
  String get artifactVersions => '版本';

  @override
  String get artifactFormRequired => '请填写必填项';

  @override
  String get artifactFormInvalid => '无法提交回答';

  @override
  String get artifactRenderFailed => '无法渲染图表';

  @override
  String artifactReply(String id, String revision, String title, String form) {
    return '关于「$title」v$revision（$id/$form）的回复';
  }

  @override
  String get connectionRouteP2p => 'P2P';

  @override
  String get connectionRouteRelay => '中继';

  @override
  String get connectionRouteUnknown => '—';

  @override
  String get backgroundNoticeTitle => '后台结果';

  @override
  String get backgroundCompleted => '已完成';

  @override
  String get backgroundFailed => '失败';

  @override
  String get backgroundCancelled => '已取消';

  @override
  String get backgroundStopped => '已停止';

  @override
  String get backgroundTimedOut => '超时';

  @override
  String get backgroundInterrupted => '已中断';

  @override
  String get backgroundStalled => '近期无输出';

  @override
  String get backgroundUnknown => '结果不确定';

  @override
  String conversationSteps(String count) {
    return '$count 个步骤';
  }

  @override
  String get conversationStep => '1 个步骤';

  @override
  String get activityProgress => '任务进度';

  @override
  String get activityBackground => '后台任务';

  @override
  String get activityPorts => '端口';

  @override
  String get hostConnecting => '连接中';

  @override
  String get hostInfo => '主机信息';

  @override
  String get hostLatency => '延迟';

  @override
  String hostLatencySummary(String average, String peak) {
    return '平均 $average　最高 $peak';
  }

  @override
  String get hostSystem => '系统';

  @override
  String get hostProcessor => '处理器';

  @override
  String hostCores(String count) {
    return '$count 核';
  }

  @override
  String hostLoad(String value) {
    return '负载 $value';
  }

  @override
  String hostMemoryShare(String percent) {
    return '已用 $percent';
  }

  @override
  String get hostUptime => '运行时间';

  @override
  String hostNodeUptime(String duration) {
    return '服务 $duration';
  }

  @override
  String hostDays(String count) {
    return '$count 天';
  }

  @override
  String hostHours(String count) {
    return '$count 小时';
  }

  @override
  String hostMinutes(String count) {
    return '$count 分钟';
  }

  @override
  String get hostTrend => '趋势';

  @override
  String get hostTrendWindow => '最近 1 分钟';

  @override
  String get hostChartStart => '60 秒前';

  @override
  String get hostChartNow => '现在';

  @override
  String get hostMetricGpu => 'GPU';

  @override
  String get hostMemoryUsed => '已用';

  @override
  String get hostMemoryCache => '缓存';

  @override
  String get hostMemorySwap => '交换';

  @override
  String get hostGpuUsage => '使用率';

  @override
  String get hostNetwork => '网络';

  @override
  String hostNetworkTotals(String received, String sent) {
    return '累计 ↓ $received　↑ $sent';
  }

  @override
  String get hostDownload => '下载';

  @override
  String get hostUpload => '上传';

  @override
  String hostPartitions(String count) {
    return '$count 个分区';
  }

  @override
  String get hostDiskRead => '读取';

  @override
  String get hostDiskWrite => '写入';

  @override
  String get hostProcesses => '进程';

  @override
  String get hostSortCpu => '按 CPU';

  @override
  String get hostSortMemory => '按内存';

  @override
  String get hostSortRecent => '最近使用';

  @override
  String get hostSearchProcesses => '搜索进程';

  @override
  String get hostSearchProjects => '搜索项目';

  @override
  String hostProcessLimit(String count) {
    return '显示前 $count 个';
  }

  @override
  String get hostNoMatches => '无匹配结果';

  @override
  String get modelFeatureOff => '标准';

  @override
  String get modelFeatureFast => '快速';

  @override
  String get modelFeatureSpeed => '速度';

  @override
  String get backgroundHeld => '结果待送达';

  @override
  String get backgroundHoldLimit => '未送达，已达自动轮次上限';

  @override
  String get backgroundHoldStopped => '未送达，回复已停止';

  @override
  String get backgroundHoldInterrupted => '未送达，回复已中断';

  @override
  String get backgroundHoldFailed => '未送达，回复失败';

  @override
  String get continuationRefused => '未继续执行';

  @override
  String get continuationPlan => '计划模式下不继续执行';

  @override
  String get continuationChild => '子任务不继续执行';

  @override
  String get continuationArchived => '已归档会话不继续执行';

  @override
  String get continuationWorktree => '未继续执行，工作树不存在';

  @override
  String get continuationQueuePaused => '未继续执行，队列已暂停';

  @override
  String get continuationQueuedInput => '未继续执行，队列中有消息';

  @override
  String get continuationRunning => '未继续执行，回复进行中';

  @override
  String get continuationInteraction => '未继续执行，有请求等待你处理';

  @override
  String get continuationLimit => '已达自动轮次上限';

  @override
  String continuationLimitCount(String count, String limit) {
    return '已达自动轮次上限 · $count/$limit';
  }

  @override
  String get continuationStale => '未继续执行，会话已有新进展';

  @override
  String get conversationContextOverflow => '上下文超出模型限制';

  @override
  String get conversationOutputLimit => '回复已达输出上限';

  @override
  String get demoHost => 'MacBook Pro';

  @override
  String get demoUnavailable => '当前无法执行此操作。';

  @override
  String get demoConversationTitle => '优化登录表单';

  @override
  String get demoNotesTitle => '准备发布评审';

  @override
  String get demoUserMessage => '优化登录表单的间距与键盘焦点，保留现有布局。';

  @override
  String get demoReply =>
      '已调整登录表单的间距与点击区域。\n\n- 表单间距设为 20 px。\n- 按钮最小高度设为 44 px。\n- 保留字段顺序与键盘导航。\n\n```css\nmin-height: 44px;\n```\n\n变更位于 `src/pages/Login.tsx` 与 `src/styles/login.css`。';

  @override
  String get demoReadme =>
      '# Lumen Web\n\n支持响应式布局、无障碍输入与键盘导航的登录页面。\n\n## 项目结构\n\n- `src/pages/Login.tsx`：登录表单。\n- `src/styles/login.css`：布局与交互样式。\n- `docs/review.md`：发布评审清单。\n\n## 界面规范\n\n字段之间保留 20 px 间距，点击区域至少为 44 px。保留字段顺序与清晰的焦点样式。\n';

  @override
  String get demoReviewNotes =>
      '# 发布评审\n\n| 范围 | 检查内容 |\n| --- | --- |\n| 输入字段 | 检查标签与必填提示 |\n| 键盘导航 | 检查 Tab 顺序与可见焦点 |\n| 点击区域 | 登录按钮高度至少为 44 px |\n| 页面布局 | 检查窄屏与宽屏显示 |\n| 代码变更 | 发布前检查登录页与样式 |\n';

  @override
  String get demoNotesPrompt => '为登录页面整理一份简短的发布评审清单。';

  @override
  String get demoSimulatedReply =>
      '我会围绕登录页面继续完善，并保留现有布局。\n\n1. 检查字段间距与键盘焦点。\n2. 保持点击区域至少为 44 px。\n3. 对照检查 `src/pages/Login.tsx` 与 `src/styles/login.css`。\n\n发布评审清单位于 `docs/review.md`。';

  @override
  String get demoTerminalWelcome =>
      'Last login: Sun Nov 15 09:00:00 on ttys001';

  @override
  String get personalKvConflict => '个人数据冲突，已保留控制端当前值';
}

/// The translations for Chinese, using the Han script (`zh_Hant`).
class AppLocalizationsZhHant extends AppLocalizationsZh {
  AppLocalizationsZhHant() : super('zh_Hant');

  @override
  String get guidanceWithdraw => '撤回引導';

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
  String get conversationStepSkipped => '已跳過';

  @override
  String get conversationChild => '子任務';

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
  String get conversationContinued => '已接續';

  @override
  String get conversationFailedStatus => '處理失敗';

  @override
  String get conversationStopping => '正在停止';

  @override
  String get conversationQueued => '等待處理';

  @override
  String get conversationGenerating => '正在回覆';

  @override
  String get conversationWaiting => '等待確認';

  @override
  String get conversationWaitingChild => '等待 1 個子代理';

  @override
  String conversationWaitingChildren(String count) {
    return '等待 $count 個子代理';
  }

  @override
  String get conversationCompacting => '正在壓縮上下文';

  @override
  String get conversationWaitingController => '等待控制端更新登入';

  @override
  String get conversationCompacted => '上下文已壓縮';

  @override
  String get conversationToolWaiting => '等待執行';

  @override
  String get conversationStartCoding => '開始執行';

  @override
  String get conversationPlanFeedback => '修改建議';

  @override
  String get conversationOther => '其他';

  @override
  String get conversationSubmit => '提交';

  @override
  String get conversationMode => '工作模式';

  @override
  String get conversationCode => '標準';

  @override
  String get conversationPlan => '計畫';

  @override
  String get conversationFlagship => '協作';

  @override
  String get conversationAsk => '每次詢問';

  @override
  String get conversationProject => '專案內';

  @override
  String get conversationFull => '完全訪問';

  @override
  String get conversationPermission => '權限';

  @override
  String get conversationAskDescription => '修改檔案和執行命令前詢問';

  @override
  String get conversationProjectDescription => '自動修改專案檔案，執行命令前詢問';

  @override
  String get conversationFullDescription => '自動修改檔案和執行命令，可影響專案外檔案';

  @override
  String get conversationBudget => '思考預算';

  @override
  String get conversationAttachment => '附件';

  @override
  String get conversationImageFailed => '圖片無法顯示';

  @override
  String get conversationImageUnavailable => '圖片無法使用';

  @override
  String get conversationDownloadFailed => '附件載入失敗';

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
  String get turnChanges => '本輪變更';

  @override
  String turnChangesCount(String count) {
    return '$count 個檔案';
  }

  @override
  String get fileChangesApprovalOne => '將修補程式套用到 1 個檔案';

  @override
  String fileChangesApproval(String count) {
    return '將修補程式套用到 $count 個檔案';
  }

  @override
  String get fileChangesAdded => '新增';

  @override
  String get fileChangesModified => '修改';

  @override
  String get fileChangesRenamed => '重新命名';

  @override
  String get fileChangesDeleted => '刪除';

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
  String get messageRewindConfirm => '保留此回合並移除後續訊息？檔案不變';

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
    return '將“$name”移到主機垃圾桶？未儲存的修改也會丟棄';
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
  String get resourceTerminalEnded => '終端已結束';

  @override
  String get resourceTerminalConnecting => '正在連線終端';

  @override
  String get resourceTerminalPaste => '貼上';

  @override
  String get resourceGitNotRepository => '此目錄不是 Git 儲存庫';

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
  String get unassignedChats => '對話';

  @override
  String get allWorktrees => '全部工作樹';

  @override
  String get tasksActive => '進行中';

  @override
  String get tasksToday => '今天';

  @override
  String get yesterday => '昨天';

  @override
  String get tasksThisWeek => '本週';

  @override
  String get tasksEarlier => '更早';

  @override
  String get filterProjects => '篩選專案';

  @override
  String get archiveShort => '歸檔';

  @override
  String get archiveTab => '歸檔';

  @override
  String get delete => '刪除';

  @override
  String get deleteTask => '刪除會話';

  @override
  String get deleteWarning => '刪除後無法繼續此會話';

  @override
  String get restored => '已恢復到首頁';

  @override
  String get restore => '恢復';

  @override
  String get archiveEmpty => '尚無已歸檔對話';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => '行動工作區';

  @override
  String get preview => '預覽';

  @override
  String get mixed => '深淺對照';

  @override
  String get dark => '深色';

  @override
  String get light => '淺色';

  @override
  String get brandColor => '主題色';

  @override
  String get brandOrange => '橙色';

  @override
  String get brandBlue => '藍色';

  @override
  String get brandGreen => '綠色';

  @override
  String get brandPurple => '紫色';

  @override
  String get brandPink => '粉色';

  @override
  String get brandTeal => '青色';

  @override
  String get focus => '單屏體驗';

  @override
  String get page => '選擇頁面';

  @override
  String get design => '功能與設計';

  @override
  String get footer => 'SAILRY / 移動端';

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
  String get allHosts => '全部主機';

  @override
  String get all => '全部';

  @override
  String get running => '執行中';

  @override
  String get waiting => '待處理';

  @override
  String get completed => '已完成';

  @override
  String get search => '搜尋';

  @override
  String get searchTasks => '搜尋任務、專案';

  @override
  String get noResults => '沒有匹配的任務';

  @override
  String get notification => '通知';

  @override
  String get newConversation => '新會話';

  @override
  String get question => '文件使用哪種語言？';

  @override
  String get reply => '回覆';

  @override
  String get approval => '審批';

  @override
  String get allow => '允許一次';

  @override
  String get deny => '拒絕';

  @override
  String get working => '正在處理';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '今天 09:36';

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
  String get questionPending => '等待你的回覆';

  @override
  String queueCount(String count) {
    return '$count 條排隊訊息';
  }

  @override
  String queuePaused(String count) {
    return '佇列已暫停 · $count';
  }

  @override
  String queueGuidance(String queue) {
    return '$queue，引導等待加入';
  }

  @override
  String get pauseQueue => '暫停佇列';

  @override
  String get resumeQueue => '繼續佇列';

  @override
  String get moveUp => '上移';

  @override
  String get thinkingNow => '正在思考';

  @override
  String get thought => '思考過程';

  @override
  String get describeTask => '描述你的任務';

  @override
  String get send => '傳送';

  @override
  String get attach => '新增附件';

  @override
  String get voice => '語音輸入';

  @override
  String get attachment => '附件';

  @override
  String get removeAttachment => '移除附件';

  @override
  String get model => '模型';

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
  String get queue => '訊息佇列';

  @override
  String get queueEmpty => '暫無排隊訊息';

  @override
  String get fork => '分支會話';

  @override
  String get stop => '停止任務';

  @override
  String get stoppedStatus => '已停止';

  @override
  String get pair => '連線主機';

  @override
  String get online => '線上';

  @override
  String get offline => '離線';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => '記憶體';

  @override
  String get metrics => '資源使用';

  @override
  String get activity => '任務活動';

  @override
  String get processes => '程序';

  @override
  String get network => '網路';

  @override
  String get relay => '中繼';

  @override
  String get retry => '重試';

  @override
  String get pairTitle => '連線一臺主機';

  @override
  String get pairCode => '配對碼';

  @override
  String get pairInvalid => '請輸入 6 位數字';

  @override
  String get workspace => '工作區';

  @override
  String get selectHost => '選擇主機';

  @override
  String get confirm => '確認';

  @override
  String get git => 'Git';

  @override
  String get gitBranches => '分支';

  @override
  String get gitHistory => '歷史';

  @override
  String gitViewAll(String count) {
    return '查看全部 $count 個變更';
  }

  @override
  String terminalRunningCount(String count) {
    return '$count 個執行中';
  }

  @override
  String get gitUnstaged => '未暫存';

  @override
  String get gitStageFile => '暫存';

  @override
  String gitCommitCount(String count) {
    return '提交 $count 個';
  }

  @override
  String get gitRemote => '遠端';

  @override
  String get gitCurrentShort => '目前';

  @override
  String gitDiscardConfirm(String name) {
    return '捨棄「$name」的全部修改？';
  }

  @override
  String gitTrashConfirm(String name) {
    return '將「$name」移到主機垃圾桶？';
  }

  @override
  String get gitCommitOptions => '提交選項';

  @override
  String get gitSkipHooks => '跳過 Hook';

  @override
  String get gitPushAfterCommit => '提交後推送';

  @override
  String get fileShare => '分享';

  @override
  String get fileTrash => '移到垃圾桶';

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
  String get review => '評審';

  @override
  String get selectWorkspace => '專案與工作樹';

  @override
  String get searchFiles => '搜尋檔案';

  @override
  String get src => '原始碼';

  @override
  String get folder => '資料夾';

  @override
  String get modified => '已修改';

  @override
  String get edit => '編輯';

  @override
  String get unsaved => '未儲存';

  @override
  String get discard => '放棄修改';

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
  String get openFile => '開啟檔案';

  @override
  String get closePort => '關閉轉發';

  @override
  String get workingTree => '工作區';

  @override
  String get staged => '已暫存';

  @override
  String get stage => '暫存全部';

  @override
  String get unstage => '取消暫存';

  @override
  String get commit => '提交';

  @override
  String get commitMessage => '提交說明';

  @override
  String get noChanges => '沒有待提交的變更';

  @override
  String get terminalKeyboard => '鍵盤';

  @override
  String get terminalEnter => '回車';

  @override
  String get readOnly => '只讀觀察';

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
  String get estimatedCost => '估算費用（USD）';

  @override
  String get partial => '部分資料';

  @override
  String get input => '輸入';

  @override
  String get output => '輸出';

  @override
  String get cached => '快取命中';

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
  String get providers => '模型與供應商';

  @override
  String get speech => '語音';

  @override
  String get about => '關於 Sailry';

  @override
  String get modelPicker => '選擇模型';

  @override
  String get team => '團隊';

  @override
  String get executionModel => '單一模型';

  @override
  String get teamDefault => '預設團隊';

  @override
  String get teamLead => '負責人';

  @override
  String get memberReset => '重設模型';

  @override
  String get auto => '自動';

  @override
  String get manual => '每次詢問';

  @override
  String get required => '請先填寫任務';

  @override
  String get hostConnectPrompt => '請連線主機';

  @override
  String get hostDisconnected => '主機暫時離線';

  @override
  String get hostUpdateRequired => '請更新主機後再連線';

  @override
  String get appUpdateRequired => '請更新此應用程式後再連線';

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
  String get preferencesFailed => '偏好未儲存，請重試';

  @override
  String get startupFailed => '啟動失敗';

  @override
  String get pairAction => '連線';

  @override
  String get pairFailed => '連線失敗，請重試';

  @override
  String get pairExpired => '配對碼已失效，請獲取新碼';

  @override
  String get pairing => '正在連線';

  @override
  String get hostRemove => '移除主機';

  @override
  String hostRemoveConfirm(String name) {
    return '移除「$name」並撤銷此裝置的存取權？主機資料保留；若這是最後一台裝置，需要管理員恢復存取。';
  }

  @override
  String get hostRemoved => '已移除主機';

  @override
  String get hostOriginDirect => '直接配對';

  @override
  String hostOriginSource(String name) {
    return '來自 $name';
  }

  @override
  String get hostRemoveFailed => '無法移除主機';

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
  String get failureDenied => '沒有操作權限';

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
  String get settingsLoadFailed => '讀取失敗';

  @override
  String get settingsSaveFailed => '儲存失敗，草稿已保留';

  @override
  String get settingsConflict => '設定已更新，請重新開啟後重試';

  @override
  String get settingsUnknown => '結果未確認，請先重新整理檢視';

  @override
  String get settingsRequired => '請填寫內容';

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
  String get apiChatCompletions => 'Chat Completions';

  @override
  String get apiResponses => 'Responses';

  @override
  String get apiAnthropic => 'Anthropic';

  @override
  String get apiGemini => 'Gemini';

  @override
  String get apiDeepSeek => 'DeepSeek';

  @override
  String get apiAzureOpenAi => 'Azure OpenAI';

  @override
  String get apiAzureAi => 'Azure AI';

  @override
  String get apiPlugin => '外掛';

  @override
  String get providerNew => '新增供應商';

  @override
  String get providerEmpty => '尚無供應商';

  @override
  String get providerDesktopHint => 'OAuth 模型使用桌面端操作';

  @override
  String get providerConnection => '連線';

  @override
  String get providerKeyUnset => '未設定';

  @override
  String get providerShowKey => '顯示金鑰';

  @override
  String get providerHideKey => '隱藏金鑰';

  @override
  String providerModelCount(String count) {
    return '$count 個模型';
  }

  @override
  String get providerFetch => '取得';

  @override
  String get providerFetchTitle => '取得模型';

  @override
  String providerFetched(String count) {
    return '介面回傳 $count 個';
  }

  @override
  String get providerSelectAll => '全選';

  @override
  String get providerSelectNone => '取消全選';

  @override
  String get providerSearch => '搜尋模型';

  @override
  String get providerAdded => '已新增';

  @override
  String providerContext(String tokens) {
    return '$tokens 上下文';
  }

  @override
  String providerAddCount(String count) {
    return '新增 $count 個';
  }

  @override
  String get providerModelId => '模型 ID';

  @override
  String get providerModelExists => '模型已存在';

  @override
  String get providerDefault => '預設';

  @override
  String get providerBasic => '基本';

  @override
  String get providerContextLength => '上下文長度';

  @override
  String get providerCapabilities => '能力';

  @override
  String get providerVision => '圖片';

  @override
  String get providerTools => '工具';

  @override
  String get providerReasoning => '推理';

  @override
  String get providerCapUnderstands => '理解';

  @override
  String get providerCapCan => '支援';

  @override
  String get providerCapGenerates => '生成';

  @override
  String get providerGenerateImage => '圖片';

  @override
  String get providerGenerateVideo => '影片';

  @override
  String get providerGenerateSpeech => '語音';

  @override
  String get providerDeploymentId => '部署名稱';

  @override
  String get providerHeaders => '自訂請求標頭';

  @override
  String get providerHeadersHint => '每行一個，格式為 名稱: 值';

  @override
  String get providerHeadersInvalid => '每行請求標頭都需要名稱和冒號';

  @override
  String get providerEfforts => '思考強度';

  @override
  String get providerEffortPlaceholder => '強度或 Token 預算';

  @override
  String get providerEffortInvalid => '思考強度無效，請檢查名稱、重複項和 API 支援';

  @override
  String get providerEffortDefault => '預設強度';

  @override
  String get providerEffortSetDefault => '設為預設';

  @override
  String get providerSetDefault => '設為預設模型';

  @override
  String get providerIsDefault => '預設模型';

  @override
  String get providerDeleteModel => '刪除模型';

  @override
  String get providerDelete => '刪除供應商';

  @override
  String get providerNumberInvalid => '請輸入正整數';

  @override
  String get providerLimitsInvalid => '最大輸出不能超過上下文長度';

  @override
  String get settingsUsageUnknown => '未知';

  @override
  String get settingsUsagePartial => '部分主機資料未更新';

  @override
  String get settingsUsageCache => '快取';

  @override
  String get settingsUsageInput => '未快取';

  @override
  String get settingsUsageOutput => '輸出';

  @override
  String get usageRange24h => '24 小時';

  @override
  String get usageRange7d => '7 天';

  @override
  String get usageRange30d => '30 天';

  @override
  String get usageRange90d => '90 天';

  @override
  String get usageRange1y => '1 年';

  @override
  String get usageReasoning => '推理';

  @override
  String get usageHitRate => '快取命中率';

  @override
  String get usageSpeed => '平均速度';

  @override
  String get usageSpeedUnit => 'tok/s';

  @override
  String get usageCostPartial => '部分';

  @override
  String get usageTrend => 'Token 趨勢';

  @override
  String get usageLocalTime => '依本地時間';

  @override
  String usageActiveDays(String count) {
    return '$count 個活躍日';
  }

  @override
  String get usageBreakdown => '分布';

  @override
  String get usageProvider => '供應商';

  @override
  String get usageShowAll => '顯示全部';

  @override
  String get usageCollapse => '收合';

  @override
  String get usageUnassigned => '未分類';

  @override
  String usageHit(String value) {
    return '命中 $value';
  }

  @override
  String usageResponseCount(String count) {
    return '$count 響應';
  }

  @override
  String get artifactContents => '目錄';

  @override
  String get artifactZoomIn => '放大';

  @override
  String get artifactZoomOut => '縮小';

  @override
  String get artifactVersions => '版本';

  @override
  String get artifactFormRequired => '請填寫必填項';

  @override
  String get artifactFormInvalid => '無法提交回答';

  @override
  String get artifactRenderFailed => '無法呈現圖表';

  @override
  String artifactReply(String id, String revision, String title, String form) {
    return '關於「$title」v$revision（$id/$form）的回覆';
  }

  @override
  String get connectionRouteP2p => 'P2P';

  @override
  String get connectionRouteRelay => '中繼';

  @override
  String get connectionRouteUnknown => '—';

  @override
  String get backgroundNoticeTitle => '背景結果';

  @override
  String get backgroundCompleted => '已完成';

  @override
  String get backgroundFailed => '失敗';

  @override
  String get backgroundCancelled => '已取消';

  @override
  String get backgroundStopped => '已停止';

  @override
  String get backgroundTimedOut => '逾時';

  @override
  String get backgroundInterrupted => '已中斷';

  @override
  String get backgroundStalled => '近期無輸出';

  @override
  String get backgroundUnknown => '結果不確定';

  @override
  String conversationSteps(String count) {
    return '$count 個步驟';
  }

  @override
  String get conversationStep => '1 個步驟';

  @override
  String get activityProgress => '任務進度';

  @override
  String get activityBackground => '背景任務';

  @override
  String get activityPorts => '連接埠';

  @override
  String get hostConnecting => '連線中';

  @override
  String get hostInfo => '主機資訊';

  @override
  String get hostLatency => '延遲';

  @override
  String hostLatencySummary(String average, String peak) {
    return '平均 $average　最高 $peak';
  }

  @override
  String get hostSystem => '系統';

  @override
  String get hostProcessor => '處理器';

  @override
  String hostCores(String count) {
    return '$count 核';
  }

  @override
  String hostLoad(String value) {
    return '負載 $value';
  }

  @override
  String hostMemoryShare(String percent) {
    return '已用 $percent';
  }

  @override
  String get hostUptime => '執行時間';

  @override
  String hostNodeUptime(String duration) {
    return '服務 $duration';
  }

  @override
  String hostDays(String count) {
    return '$count 天';
  }

  @override
  String hostHours(String count) {
    return '$count 小時';
  }

  @override
  String hostMinutes(String count) {
    return '$count 分鐘';
  }

  @override
  String get hostTrend => '趨勢';

  @override
  String get hostTrendWindow => '最近 1 分鐘';

  @override
  String get hostChartStart => '60s 前';

  @override
  String get hostChartNow => '現在';

  @override
  String get hostMetricGpu => 'GPU';

  @override
  String get hostMemoryUsed => '已用';

  @override
  String get hostMemoryCache => '快取';

  @override
  String get hostMemorySwap => '交換';

  @override
  String get hostGpuUsage => '使用率';

  @override
  String get hostNetwork => '網路';

  @override
  String hostNetworkTotals(String received, String sent) {
    return '累計 ↓ $received　↑ $sent';
  }

  @override
  String get hostDownload => '下載';

  @override
  String get hostUpload => '上傳';

  @override
  String hostPartitions(String count) {
    return '$count 個分割區';
  }

  @override
  String get hostDiskRead => '讀取';

  @override
  String get hostDiskWrite => '寫入';

  @override
  String get hostProcesses => '程序';

  @override
  String get hostSortCpu => '依 CPU';

  @override
  String get hostSortMemory => '依記憶體';

  @override
  String get hostSortRecent => '最近使用';

  @override
  String get hostSearchProcesses => '搜尋程序';

  @override
  String get hostSearchProjects => '搜尋專案';

  @override
  String hostProcessLimit(String count) {
    return '顯示前 $count 個';
  }

  @override
  String get hostNoMatches => '無相符結果';

  @override
  String get modelFeatureOff => '標準';

  @override
  String get modelFeatureFast => '快速';

  @override
  String get modelFeatureSpeed => '速度';

  @override
  String get backgroundHeld => '結果待送達';

  @override
  String get backgroundHoldLimit => '未送達，已達自動輪次上限';

  @override
  String get backgroundHoldStopped => '未送達，回覆已停止';

  @override
  String get backgroundHoldInterrupted => '未送達，回覆已中斷';

  @override
  String get backgroundHoldFailed => '未送達，回覆失敗';

  @override
  String get continuationRefused => '未繼續執行';

  @override
  String get continuationPlan => '計畫模式下不繼續執行';

  @override
  String get continuationChild => '子任務不繼續執行';

  @override
  String get continuationArchived => '已歸檔會話不繼續執行';

  @override
  String get continuationWorktree => '未繼續執行，工作樹不存在';

  @override
  String get continuationQueuePaused => '未繼續執行，佇列已暫停';

  @override
  String get continuationQueuedInput => '未繼續執行，佇列中有訊息';

  @override
  String get continuationRunning => '未繼續執行，回覆進行中';

  @override
  String get continuationInteraction => '未繼續執行，有請求等待你處理';

  @override
  String get continuationLimit => '已達自動輪次上限';

  @override
  String continuationLimitCount(String count, String limit) {
    return '已達自動輪次上限 · $count/$limit';
  }

  @override
  String get continuationStale => '未繼續執行，會話已有新進展';

  @override
  String get conversationContextOverflow => '上下文超出模型限制';

  @override
  String get conversationOutputLimit => '回覆已達輸出上限';

  @override
  String get demoHost => 'MacBook Pro';

  @override
  String get demoUnavailable => '目前無法執行此操作。';

  @override
  String get demoConversationTitle => '最佳化登入表單';

  @override
  String get demoNotesTitle => '準備發布評審';

  @override
  String get demoUserMessage => '最佳化登入表單的間距與鍵盤焦點，保留現有版面配置。';

  @override
  String get demoReply =>
      '已調整登入表單的間距與點選區域。\n\n- 表單間距設為 20 px。\n- 按鈕最小高度設為 44 px。\n- 保留欄位順序與鍵盤導覽。\n\n```css\nmin-height: 44px;\n```\n\n變更位於 `src/pages/Login.tsx` 與 `src/styles/login.css`。';

  @override
  String get demoReadme =>
      '# Lumen Web\n\n支援響應式版面配置、無障礙輸入與鍵盤導覽的登入頁面。\n\n## 專案結構\n\n- `src/pages/Login.tsx`：登入表單。\n- `src/styles/login.css`：版面配置與互動樣式。\n- `docs/review.md`：發布評審清單。\n\n## 介面規範\n\n欄位之間保留 20 px 間距，點選區域至少為 44 px。保留欄位順序與清晰的焦點樣式。\n';

  @override
  String get demoReviewNotes =>
      '# 發布評審\n\n| 範圍 | 檢查內容 |\n| --- | --- |\n| 輸入欄位 | 檢查標籤與必填提示 |\n| 鍵盤導覽 | 檢查 Tab 順序與可見焦點 |\n| 點選區域 | 登入按鈕高度至少為 44 px |\n| 頁面配置 | 檢查窄螢幕與寬螢幕顯示 |\n| 程式碼變更 | 發布前檢查登入頁面與樣式 |\n';

  @override
  String get demoNotesPrompt => '為登入頁面整理一份簡短的發布評審清單。';

  @override
  String get demoSimulatedReply =>
      '我會圍繞登入頁面繼續完善，並保留現有版面配置。\n\n1. 檢查欄位間距與鍵盤焦點。\n2. 保持點選區域至少為 44 px。\n3. 對照檢查 `src/pages/Login.tsx` 與 `src/styles/login.css`。\n\n發布評審清單位於 `docs/review.md`。';

  @override
  String get demoTerminalWelcome =>
      'Last login: Sun Nov 15 09:00:00 on ttys001';

  @override
  String get personalKvConflict => '個人資料衝突，已保留控制端目前的值';
}
