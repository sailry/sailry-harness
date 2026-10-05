// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for English (`en`).
class AppLocalizationsEn extends AppLocalizations {
  AppLocalizationsEn([String locale = 'en']) : super(locale);

  @override
  String get retryTask => 'Retry';

  @override
  String get welcomeTitle => 'What would you like to work on today?';

  @override
  String get welcomeExplore => 'Explore a project';

  @override
  String get welcomeExploreDetail =>
      'Understand its structure and entry points';

  @override
  String get welcomeExplorePrompt =>
      'Help me understand this project, including its key modules and entry points.';

  @override
  String get welcomeBuild => 'Build an idea';

  @override
  String get welcomeBuildDetail => 'Bring your idea to life';

  @override
  String get welcomeBuildPrompt =>
      'I want to add a feature to this project. First confirm the requirements with me and outline an implementation plan.';

  @override
  String get welcomeReview => 'Review changes';

  @override
  String get welcomeReviewDetail => 'Check changes and potential issues';

  @override
  String get welcomeReviewPrompt =>
      'Review the current changes in this project, focusing on potential issues and missing tests.';

  @override
  String get welcomePlan => 'Make a plan';

  @override
  String get welcomePlanDetail => 'Clarify goals and steps';

  @override
  String get welcomePlanPrompt =>
      'Help me create a step-by-step plan for the upcoming development work.';

  @override
  String get conversationEmpty => 'Describe your task';

  @override
  String get conversationLoading => 'Loading chat';

  @override
  String get conversationReconnecting => 'Reconnecting';

  @override
  String get conversationErrorDetails => 'View reason';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Retrying model request $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Model request retried $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count tool calls';
  }

  @override
  String get conversationGoal => 'Goal';

  @override
  String get conversationGoalBlocked => 'Blocked';

  @override
  String conversationGoalBudget(String count) {
    return 'Budget $count tokens';
  }

  @override
  String get conversationStepSkipped => 'Skipped';

  @override
  String get conversationChild => 'Subtask';

  @override
  String get conversationChildReadonly => 'Subtask chat';

  @override
  String get conversationOffline => 'Connection lost';

  @override
  String get conversationUnavailable => 'Chat unavailable';

  @override
  String get conversationFailed => 'Operation failed; retry';

  @override
  String get conversationUnknown =>
      'Outcome unconfirmed; check the chat before acting again';

  @override
  String get conversationCheckResult => 'Check outcome';

  @override
  String get conversationConflict =>
      'Configuration changed; reopen before acting again';

  @override
  String get conversationOlder => 'Load older messages';

  @override
  String get conversationNew => 'New chat';

  @override
  String get conversationNoHost => 'Connect a host first';

  @override
  String get conversationNoProject => 'Add a project on the host first';

  @override
  String get conversationNoModel => 'Configure a model on the host first';

  @override
  String get conversationNoTasks => 'No chats yet';

  @override
  String get conversationNoMessages => 'No messages';

  @override
  String get conversationPreviewUnavailable => 'Message unavailable';

  @override
  String get conversationInterrupted => 'Interrupted';

  @override
  String get conversationFailedStatus => 'Failed';

  @override
  String get conversationStopping => 'Stopping';

  @override
  String get conversationQueued => 'Queued';

  @override
  String get conversationProcessing => 'Processing';

  @override
  String get conversationUnsynced => 'State not synced';

  @override
  String get conversationGenerating => 'Responding';

  @override
  String get conversationWaiting => 'Awaiting confirmation';

  @override
  String get conversationCompacting => 'Compacting context';

  @override
  String get conversationForkConfirm => 'Fork a chat from this record?';

  @override
  String get conversationCompacted => 'Context compacted';

  @override
  String get conversationToolWaiting => 'Pending';

  @override
  String get conversationToolRunning => 'Running';

  @override
  String get conversationToolReturned => 'Returned';

  @override
  String get conversationToolCancelled => 'Cancelled';

  @override
  String get conversationToolNotExecuted => 'Not executed';

  @override
  String get conversationToolInterrupted => 'Interrupted';

  @override
  String get conversationUnsupportedInput => 'Handle this input on desktop';

  @override
  String get conversationStartCoding => 'Start execution';

  @override
  String get conversationPlanFeedback => 'Suggest changes';

  @override
  String get conversationOther => 'Other';

  @override
  String get conversationSubmit => 'Submit';

  @override
  String get conversationSource => 'Source';

  @override
  String get conversationMode => 'Work mode';

  @override
  String get conversationCode => 'Execute';

  @override
  String get conversationPlan => 'Plan';

  @override
  String get conversationPermission => 'Permissions';

  @override
  String get conversationAsk => 'Ask each time';

  @override
  String get conversationProject => 'Project access';

  @override
  String get conversationFull => 'Full access';

  @override
  String get conversationReasoning => 'Reasoning effort';

  @override
  String get conversationDefault => 'Default';

  @override
  String get conversationNone => 'Off';

  @override
  String get conversationMinimal => 'Minimal';

  @override
  String get conversationLow => 'Low';

  @override
  String get conversationMedium => 'Medium';

  @override
  String get conversationHigh => 'High';

  @override
  String get conversationXHigh => 'Higher';

  @override
  String get conversationMax => 'Maximum';

  @override
  String get conversationBudget => 'Reasoning budget';

  @override
  String get conversationAttachment => 'Attachment';

  @override
  String get conversationAttachmentTooLarge =>
      'Attachment unreadable or larger than 64 MB';

  @override
  String get conversationDownload => 'View attachment';

  @override
  String get conversationImageFailed => 'Cannot display image';

  @override
  String get conversationDownloadFailed => 'Could not load attachment';

  @override
  String get conversationReadonly => 'This chat is archived';

  @override
  String get conversationMicrophoneDenied => 'Cannot access microphone';

  @override
  String get conversationRecordingFailed => 'Speech recognition failed';

  @override
  String get conversationSpeechDisabled => 'Voice input is off';

  @override
  String get conversationSpeechMissing =>
      'Download the speech model in Settings first';

  @override
  String get conversationRecording => 'Recording';

  @override
  String get conversationTranscribing => 'Transcribing';

  @override
  String get conversationRecordReady => 'Ready to record';

  @override
  String get conversationStartRecording => 'Start recording';

  @override
  String get conversationFinishRecording => 'Finish recording';

  @override
  String get conversationSources => 'Sources';

  @override
  String get conversationSearchSuggestions => 'Search suggestions';

  @override
  String get conversationStats => 'Chat usage';

  @override
  String get conversationStatsEmpty => 'No usage yet';

  @override
  String get conversationStatsOverview => 'Overview';

  @override
  String get conversationStatsTokenGroup => 'Token usage';

  @override
  String get conversationStatsCostGroup => 'Cost';

  @override
  String get conversationStatsGenerationGroup => 'Generation';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Input';

  @override
  String get conversationStatsOutput => 'Output';

  @override
  String get conversationStatsCached => 'Cached input';

  @override
  String get conversationStatsReasoning => 'Reasoning output';

  @override
  String get conversationStatsCacheRate => 'Cache hits';

  @override
  String get conversationStatsCost => 'Estimated cost';

  @override
  String get conversationStatsCostCoverage => 'Cost coverage';

  @override
  String get conversationStatsSpeed => 'Generation speed';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Timing coverage';

  @override
  String get conversationStatsTurns => 'Turns';

  @override
  String get conversationStatsResponses => 'Model responses';

  @override
  String get conversationStatsContext => 'Current context';

  @override
  String get conversationStatsInputCost => 'Input cost';

  @override
  String get conversationStatsOutputCost => 'Output cost';

  @override
  String get conversationStatsCacheReadCost => 'Cache read cost';

  @override
  String get conversationStatsCacheWriteCost => 'Cache write cost';

  @override
  String get messageHistoryUpdated => 'Chat updated; original records kept';

  @override
  String get turnUndoUnsaved =>
      'Files have unsaved changes; save or discard them first';

  @override
  String get codePlain => 'Plain text';

  @override
  String get toolArguments => 'Arguments';

  @override
  String get toolResult => 'Result';

  @override
  String get toolRaw => 'Raw result';

  @override
  String get turnChanges => 'Turn changes';

  @override
  String turnChangesCount(String count) {
    return '$count files';
  }

  @override
  String get turnUndo => 'Undo changes';

  @override
  String get turnUndoAll => 'Undo all';

  @override
  String get turnUndoConfirm =>
      'Undo these file changes from this turn? Conflicts with later changes will stop the operation';

  @override
  String get turnUndoDone => 'Undone';

  @override
  String get turnUndoPartial =>
      'Some changes undone; check the remaining files';

  @override
  String get messageActions => 'Message actions';

  @override
  String get messageEdit => 'Edit and regenerate';

  @override
  String get messageEditConfirm =>
      'Replace this message and the following chat? Files will not be rolled back';

  @override
  String get messageRewind => 'Rewind here';

  @override
  String get messageRewindConfirm =>
      'Rewind to this turn? Later chat records will be backed up; files will not be rolled back';

  @override
  String get messageBackup => 'View chat backup';

  @override
  String get messageRegenerate => 'Regenerate';

  @override
  String get messageSearch => 'Search chat';

  @override
  String get messageSearchHint => 'Search messages';

  @override
  String get messageSearchMissing =>
      'This message is no longer in the current chat';

  @override
  String get messageSearchStale => 'Chat changed; search again';

  @override
  String get messageNoResults => 'No matching messages';

  @override
  String get messageCheck => 'Check operation outcome';

  @override
  String get messageReference => 'Reference';

  @override
  String get messageReferenceContext =>
      'This reference belongs to the context when the message was sent';

  @override
  String get toolFailed => 'Failed';

  @override
  String toolExitCode(String code) {
    return 'Exit code $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Terminated by signal $signal';
  }

  @override
  String get toolTimedOut => 'Command timed out';

  @override
  String get toolCancelled => 'Command cancelled';

  @override
  String get toolOutcomeUnknown => 'Command outcome unknown';

  @override
  String get toolQuestionAnswered => 'Answered';

  @override
  String get toolQuestionDeclined => 'Declined';

  @override
  String get toolQuestionCancelled => 'Cancelled';

  @override
  String get fileLinkUnavailable => 'Cannot open this link';

  @override
  String get imagePreview => 'Image preview';

  @override
  String get fileOpenExternal => 'Open with another app';

  @override
  String get fileOpenFailed => 'Cannot open file';

  @override
  String get fileNoApplication => 'No app can open this file';

  @override
  String get fileSaveBeforeShare => 'Save changes before sharing?';

  @override
  String fileTrashConfirm(String name) {
    return 'Move “$name” to the host trash? Unsaved changes will also be discarded';
  }

  @override
  String get fileTrashUncertain =>
      'Deletion outcome unconfirmed; retry the query';

  @override
  String get fileSaveFailed => 'File save failed';

  @override
  String get terminalHideKeyboard => 'Hide keyboard';

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
  String get terminalArrowLeft => 'Left';

  @override
  String get terminalArrowUp => 'Up';

  @override
  String get terminalArrowDown => 'Down';

  @override
  String get terminalArrowRight => 'Right';

  @override
  String get resourceNoWorkspace => 'Choose a worktree on a connected host';

  @override
  String get resourceDisconnected => 'Host not connected';

  @override
  String get resourceRoot => 'Root';

  @override
  String get resourceMore => 'Load more';

  @override
  String get resourcePartial => 'Partial content shown';

  @override
  String get resourceEmpty => 'No content';

  @override
  String get resourceSaveError => 'Save failed; draft kept';

  @override
  String get resourceReloadConfirm =>
      'Discard the draft and load the latest content?';

  @override
  String get resourceWorktreeCreate => 'New worktree';

  @override
  String get resourceSessionServices => 'Chat services';

  @override
  String get resourceServicesUnavailable => 'Service list unavailable';

  @override
  String get resourceNoServices => 'No service addresses found';

  @override
  String get resourceServiceOpen => 'Open service';

  @override
  String get resourceRemotePort => 'Remote port';

  @override
  String get resourceOpenPort => 'Forward port';

  @override
  String get resourceOpenBrowser => 'Preview web page';

  @override
  String get resourcePreviewFailed => 'Could not load page';

  @override
  String get resourcePreviewLink => 'Cannot open this link in preview';

  @override
  String get resourceForwardStopped => 'Forwarding stopped';

  @override
  String get resourceTerminalControl => 'Take control';

  @override
  String get resourceTerminalControlHint => 'Controlled by another device';

  @override
  String get resourceTerminalClaiming => 'Taking control';

  @override
  String get resourceTerminalReadOnly => 'Read-only terminal';

  @override
  String get resourceTerminalEnded => 'Terminal ended';

  @override
  String get resourceTerminalConnecting => 'Connecting terminal';

  @override
  String get resourceTerminalInput => 'Terminal input';

  @override
  String get resourceTerminalPaste => 'Paste';

  @override
  String get resourceGitNotRepository =>
      'This directory is not a Git repository';

  @override
  String get resourceInvalidPort => 'Enter a port from 1–65535';

  @override
  String get tool_navigate => 'Open page';

  @override
  String get tool_back => 'Go back';

  @override
  String get tool_forward => 'Go forward';

  @override
  String get tool_refresh => 'Refresh page';

  @override
  String get tool_right_click => 'Click element';

  @override
  String get tool_clear => 'Enter text';

  @override
  String get tool_select => 'Select option';

  @override
  String get tool_hover => 'Hover element';

  @override
  String get tool_scroll => 'Scroll page';

  @override
  String get tool_press_key => 'Press key';

  @override
  String get tool_new_tab => 'New tab';

  @override
  String get tool_list_windows => 'Browser tabs';

  @override
  String get tool_switch_window => 'Switch tab';

  @override
  String get tool_close_window => 'Close window';

  @override
  String get tool_close_session => 'Close browser';

  @override
  String get tool_screenshot => 'Capture page';

  @override
  String get tool_print_to_pdf => 'Export PDF';

  @override
  String get tool_file_upload => 'Upload file';

  @override
  String get tool_downloads => 'View downloads';

  @override
  String get tool_save_download => 'Save downloaded file';

  @override
  String get tool_evaluate_js => 'Run page script';

  @override
  String get tool_get_cookies => 'Read cookies';

  @override
  String get tool_delete_all_cookies => 'Change cookies';

  @override
  String get tool_drag_and_drop => 'Drag element';

  @override
  String get tool_focus => 'Focus element';

  @override
  String get tool_handle_alert => 'Handle page alert';

  @override
  String get tool_database_catalog => 'Browse database';

  @override
  String get tool_database_query => 'Query database';

  @override
  String get tool_database_execute => 'Execute database operation';

  @override
  String get tool_search_memory => 'Search memory';

  @override
  String get tool_review_memories => 'Review memories';

  @override
  String get tool_consolidate_memories => 'Merge memories';

  @override
  String get tool_save_memory => 'Save memory';

  @override
  String get tool_forget_memory => 'Delete memory';

  @override
  String get tool_update_plan => 'Update plan';

  @override
  String get tool_create_goal => 'Create goal';

  @override
  String get tool_get_goal => 'View goal';

  @override
  String get tool_update_goal => 'Update goal';

  @override
  String get tool_spawn_agent => 'Subagent';

  @override
  String get tool_browser_tabs => 'Browser tabs';

  @override
  String get tool_browser_read => 'Read page';

  @override
  String get tool_browser_navigate => 'Open page';

  @override
  String get tool_browser_click => 'Click element';

  @override
  String get tool_browser_input => 'Enter text';

  @override
  String get tool_browser_scroll => 'Scroll page';

  @override
  String get tool_browser_back => 'Go back';

  @override
  String get tool_browser_forward => 'Go forward';

  @override
  String get tool_browser_refresh => 'Refresh page';

  @override
  String get tool_browser_open => 'New tab';

  @override
  String get tool_browser_close => 'Close tab';

  @override
  String get tool_browser_focus => 'Switch tab';

  @override
  String get tool_browser_select => 'Select option';

  @override
  String get tool_browser_hover => 'Hover element';

  @override
  String get tool_browser_key => 'Press key';

  @override
  String get tool_browser_frame => 'Switch frame';

  @override
  String get tool_browser_wait => 'Wait for page';

  @override
  String get tool_browser_screenshot => 'Capture page';

  @override
  String get tool_ssh_run => 'Run SSH command';

  @override
  String get tool_ssh_transfer => 'Transfer SSH file';

  @override
  String get tool_list_worktrees => 'List worktrees';

  @override
  String get tool_create_worktree => 'New worktree';

  @override
  String get tool_register_worktree => 'Add worktree';

  @override
  String get tool_remove_worktree => 'Remove worktree';

  @override
  String get tool_google_search => 'Search web';

  @override
  String get tool_web_fetch => 'Fetch page';

  @override
  String get tool_fetch_url => 'Fetch page';

  @override
  String get tool_read_file => 'Read file';

  @override
  String get tool_write_file => 'Write file';

  @override
  String get tool_list_directory => 'Browse directory';

  @override
  String get tool_search_files => 'Search files';

  @override
  String get tool_run_command => 'Run command';

  @override
  String get tool_read_command => 'View background command';

  @override
  String get tool_stop_command => 'Stop command';

  @override
  String get tool_load_skill => 'Load skill';

  @override
  String get tool_read_skill_resource => 'Read skill resource';

  @override
  String get tool_computer_desktop => 'View desktop';

  @override
  String get tool_computer_observe => 'Observe screen';

  @override
  String get tool_computer_input => 'Control computer';

  @override
  String get tool_computer_focus => 'Switch app';

  @override
  String get tool_computer_open => 'Open app';

  @override
  String get tool_git_status => 'Git status';

  @override
  String get tool_git_diff => 'View diff';

  @override
  String get tool_git_log => 'Git log';

  @override
  String get tool_inspect_image => 'Inspect image';

  @override
  String get tool_generate_image => 'Generate image';

  @override
  String get tool_generate_video => 'Generate video';

  @override
  String get terminalUnavailable => 'Terminal is not connected';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Refresh';

  @override
  String get loading => 'Loading';

  @override
  String get home => 'Chats';

  @override
  String get idle => 'Idle';

  @override
  String get allProjects => 'All projects';

  @override
  String get allWorktrees => 'All worktrees';

  @override
  String get filterProjects => 'Filter projects';

  @override
  String get closeSearch => 'Close search';

  @override
  String onlineHostCount(String count) {
    return '$count online';
  }

  @override
  String get taskActions => 'Task actions';

  @override
  String get archiveShort => 'Archive';

  @override
  String get archiveTab => 'Archived';

  @override
  String get archivedTasks => 'Archived';

  @override
  String get delete => 'Delete';

  @override
  String get deleteTask => 'Delete chat';

  @override
  String get deleteWarning => 'This chat cannot be resumed after deletion';

  @override
  String get busyDelete => 'Stop the task before deleting this chat';

  @override
  String get stopBeforeDelete => 'Stop task';

  @override
  String get deleted => 'Chat removed from preview';

  @override
  String get restored => 'Restored to home';

  @override
  String get restore => 'Restore';

  @override
  String get archiveEmpty => 'No archived chats';

  @override
  String get archiveKeepsRunning => 'Archiving does not stop running tasks';

  @override
  String get title => 'Sailry · Mobile preview';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Mobile workspace';

  @override
  String get edition => 'MOBILE EXPLORATION / 01';

  @override
  String get intro => 'Tasks, chats and remote workspaces';

  @override
  String get preview => 'Preview';

  @override
  String get sample => 'Sample data · Changes stay on this page';

  @override
  String get mixed => 'Light and dark';

  @override
  String get dark => 'Dark';

  @override
  String get light => 'Light';

  @override
  String get gallery => 'Overview';

  @override
  String get focus => 'Single screen';

  @override
  String get reset => 'Reset preview';

  @override
  String get page => 'Choose page';

  @override
  String get experience => 'Open page';

  @override
  String get backGallery => 'Back to overview';

  @override
  String get design => 'Features and design';

  @override
  String get footer => 'SAILRY / MOBILE';

  @override
  String get footerNote => 'Local HTML preview · No service connection';

  @override
  String get tasks => 'Tasks';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Hosts';

  @override
  String get resources => 'Resources';

  @override
  String get settings => 'Settings';

  @override
  String get usage => 'Usage';

  @override
  String get changes => 'Changes';

  @override
  String get terminal => 'Terminal';

  @override
  String get newTerminal => 'New terminal';

  @override
  String get files => 'Files';

  @override
  String get project => 'Project';

  @override
  String get worktree => 'Worktree';

  @override
  String get subtitleTasks =>
      'Tasks across hosts · Approvals and replies first';

  @override
  String get subtitleChat => 'Continuous chat · Expand tool activity as needed';

  @override
  String get subtitleHosts => 'Connections, host resources and processes';

  @override
  String get subtitleResources => 'Host → Project → Worktree';

  @override
  String get subtitleChanges => 'File diffs, staging and commits';

  @override
  String get subtitleTerminal => 'Remote terminal · Explicit input control';

  @override
  String get subtitleUsage => 'Sailry chats · Aggregated across hosts';

  @override
  String get subtitleSettings =>
      'Local preferences and execution Node settings';

  @override
  String get allHosts => 'All hosts';

  @override
  String get connectedHosts => '2 online';

  @override
  String get all => 'All';

  @override
  String get running => 'Running';

  @override
  String get waiting => 'Pending';

  @override
  String get completed => 'Completed';

  @override
  String get taskProgress => 'Current task';

  @override
  String get taskWait => 'Awaiting your decision';

  @override
  String get taskRecent => 'Recently completed';

  @override
  String get search => 'Search';

  @override
  String get searchTasks => 'Search tasks and projects';

  @override
  String get filterTasks => 'Filter tasks';

  @override
  String get noResults => 'No matching tasks';

  @override
  String get notification => 'Notifications';

  @override
  String get newTask => 'New task';

  @override
  String get newConversation => 'New chat';

  @override
  String get approveTitle => 'Update the login layout';

  @override
  String get approveNote => 'Run project tests';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'Organize API documentation';

  @override
  String get questionNote => 'Awaiting reply';

  @override
  String get question => 'Which language should the documentation use?';

  @override
  String get optionChinese => 'Chinese';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Reply';

  @override
  String get approval => 'Approval';

  @override
  String get viewRequest => 'View request';

  @override
  String get taskSearch => 'Improve file search';

  @override
  String get taskSearchNote => 'Checking the directory index';

  @override
  String get taskTest => 'Fix chat recovery';

  @override
  String get taskTestNote => 'Running tests';

  @override
  String get taskDone => 'Update the project README';

  @override
  String get taskDoneNote => '3 files changed';

  @override
  String get ago => 'Just now';

  @override
  String get minutesAgo => '12 minutes ago';

  @override
  String get allow => 'Allow once';

  @override
  String get deny => 'Deny';

  @override
  String get approved => 'Allowed · Sample';

  @override
  String get denied => 'Denied · Sample';

  @override
  String get answered => 'Replied · Sample';

  @override
  String get awaiting => 'Awaiting approval';

  @override
  String get working => 'Working';

  @override
  String get viewChanges => 'View changes';

  @override
  String get viewConversation => 'View chat';

  @override
  String get chatTitle => 'Update the login layout';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Today 09:36';

  @override
  String get userMessage =>
      'Adjust the login spacing and unify input and button styles while preserving the login logic';

  @override
  String get assistantMessage =>
      'I checked the login page and shared form components, unified input spacing and added keyboard focus styles';

  @override
  String get replyPreview => 'Sample task flow';

  @override
  String get phaseThinking => 'Thinking';

  @override
  String get phaseReading => 'Reading files';

  @override
  String get phaseQuestion => 'Awaiting reply';

  @override
  String get phaseEditing => 'Editing files';

  @override
  String get phaseApproval => 'Awaiting approval';

  @override
  String get phaseTesting => 'Running tests';

  @override
  String get phaseReply => 'Responding';

  @override
  String get phaseFollowup => 'Processing queue';

  @override
  String get phaseComplete => 'Completed';

  @override
  String get phaseFailed => 'Tests failed';

  @override
  String get allowShort => 'Allow';

  @override
  String get queueShort => 'Queue';

  @override
  String get confirmShort => 'Confirm';

  @override
  String get todoShort => 'To do';

  @override
  String get todoInspect => 'Inspect the login page';

  @override
  String get todoEdit => 'Adjust form styles';

  @override
  String get todoTest => 'Run project tests';

  @override
  String get todoNarrow => 'Check narrow-screen spacing';

  @override
  String get workProcess => 'Activity';

  @override
  String workSteps(String count) {
    return '· $count steps';
  }

  @override
  String get questionRecord => 'Confirm layout';

  @override
  String get answerRecorded => 'Replied';

  @override
  String get playFlow => 'Play task';

  @override
  String get pauseFlow => 'Pause demo';

  @override
  String get nextFlow => 'Next step';

  @override
  String get replyingNow => 'Responding';

  @override
  String get toolReadLabel => 'Read';

  @override
  String get toolEditLabel => 'Edit';

  @override
  String get toolRunLabel => 'Run';

  @override
  String get readGroup => '3 files';

  @override
  String get readFileResult => 'File read';

  @override
  String get readFileProgress => 'Reading file';

  @override
  String get flowAttachment =>
      'Login update: unify form spacing, add keyboard focus styles and preserve login logic';

  @override
  String get readResult =>
      'Read Login.tsx and shared form styles\nThe mobile button width differs from the form';

  @override
  String get layoutFindings =>
      'The login form uses desktop spacing and the mobile button does not fill its container';

  @override
  String get layoutQuestion => 'Should the mobile login button fill the width?';

  @override
  String get questionPending => 'Awaiting your reply';

  @override
  String get wideButton => 'Use full-width button';

  @override
  String get keepButton => 'Keep current width';

  @override
  String get editPlan =>
      'I will preserve login logic, unify spacing and make the mobile button full width';

  @override
  String get editPlanKeep =>
      'I will keep the button width and login logic, adjusting only spacing and focus styles';

  @override
  String get editThinking =>
      'Reuse existing style variables and limit layout changes to the login form';

  @override
  String get editResult =>
      'Updated 3 files\nAdded focus styles and mobile layout rules';

  @override
  String get beforeTest =>
      'Layout changes are complete. Next I will run project tests to check for regressions';

  @override
  String get testTool => 'Run project tests';

  @override
  String get testProgress =>
      'Running login form tests…\nChecking focus and keyboard interaction';

  @override
  String get testResult => '12 tests passed\nNo login logic regressions found';

  @override
  String get testFailure =>
      'Focus order test failed\nExpected focus on the password field, but it remained on the username field';

  @override
  String get testFailed => 'Tests failed';

  @override
  String get flowResult =>
      'Login spacing and focus styles are unified, with a full-width mobile button. All 12 tests passed and login logic is unchanged';

  @override
  String get queueSample => 'Check the button spacing on narrow screens too';

  @override
  String queueCount(String count) {
    return '$count queued messages';
  }

  @override
  String queuePaused(String count) {
    return 'Queue paused · $count';
  }

  @override
  String get pauseQueue => 'Pause queue';

  @override
  String get resumeQueue => 'Resume queue';

  @override
  String get sendNext => 'Send next';

  @override
  String get enqueue => 'Add to queue';

  @override
  String get queuedPreview => 'Added to sample queue';

  @override
  String get moveUp => 'Move up';

  @override
  String get followupThinking =>
      'Check existing breakpoints to confirm consistent button spacing on narrow screens';

  @override
  String get followupTool => 'Check narrow-screen styles';

  @override
  String get followupToolResult => '320px and 390px use the same spacing rules';

  @override
  String get followupResult =>
      'Narrow-screen button spacing is consistent; no further changes are needed';

  @override
  String get deniedResult =>
      'Tests were not run; current changes are preserved';

  @override
  String get thinkingNow => 'Thinking';

  @override
  String get toolsNow => 'Executing';

  @override
  String get toolPending => 'Not started';

  @override
  String get thoughtLive =>
      'First inspect the login page and form components to identify spacing and focus changes';

  @override
  String get toolsShort => '3 actions';

  @override
  String get thought => 'Reasoning';

  @override
  String get thoughtContent =>
      'Reuse existing form components and adjust only the login layout and focus styles';

  @override
  String get toolsComplete => '3 actions completed';

  @override
  String get toolRead => 'Read login and form components';

  @override
  String get toolEdit => 'Update spacing and focus styles';

  @override
  String get toolDiff => 'Inspect file diffs';

  @override
  String get changedFiles => '3 files changed';

  @override
  String get approvalBody => 'Run tests in the sailry-web worktree on Studio';

  @override
  String get approvalResolved => 'Approval resolved';

  @override
  String get chatContinue => 'Continue describing your task';

  @override
  String get describeTask => 'Describe your task';

  @override
  String get send => 'Send';

  @override
  String get attach => 'Attach';

  @override
  String get voice => 'Voice input';

  @override
  String get voiceNote => 'This preview does not access the microphone';

  @override
  String get attachmentNote => 'Sample attachment added';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Remove attachment';

  @override
  String get sentPreview => 'Preview only, not sent';

  @override
  String get model => 'Model';

  @override
  String get modelSource => 'Current chat configuration · Studio';

  @override
  String get copy => 'Copy';

  @override
  String get copied => 'Copied';

  @override
  String get copyFailed => 'Copy unavailable; select the text manually';

  @override
  String get more => 'More';

  @override
  String get close => 'Close';

  @override
  String get back => 'Back';

  @override
  String get cancel => 'Cancel';

  @override
  String get save => 'Save';

  @override
  String get select => 'Select';

  @override
  String get sessionActions => 'Chat actions';

  @override
  String get queue => 'Message queue';

  @override
  String get queueEmpty => 'No queued messages';

  @override
  String get fork => 'Fork chat';

  @override
  String get forked => 'Sample fork created';

  @override
  String get archive => 'Archive chat';

  @override
  String get archived => 'Archived in preview';

  @override
  String get stop => 'Stop task';

  @override
  String get stopped => 'Task stopped in preview';

  @override
  String get stoppedStatus => 'Stopped';

  @override
  String get hostSubtitle => 'Your execution Nodes';

  @override
  String get pair => 'Connect host';

  @override
  String get online => 'Online';

  @override
  String get offline => 'Offline';

  @override
  String get connection => 'Connection';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 cores';

  @override
  String get laptopSystem => 'Last online 2 hours ago';

  @override
  String get statusHealthy => 'Healthy';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Memory';

  @override
  String get disk => 'Disk';

  @override
  String get metrics => 'Resource usage';

  @override
  String get activity => 'Activity';

  @override
  String get lastHour => 'Last 60 minutes';

  @override
  String get sessionCount => 'Chats';

  @override
  String get terminalCount => 'Terminals';

  @override
  String get projectCount => 'Projects';

  @override
  String get processes => 'Processes';

  @override
  String get process => 'Name';

  @override
  String get network => 'Network';

  @override
  String get details => 'Details';

  @override
  String get manageHost => 'Host details';

  @override
  String get hostProjects => 'Host projects';

  @override
  String get connectionDetails => 'Connection details';

  @override
  String get direct => 'Direct';

  @override
  String get relay => 'Relay';

  @override
  String get latency => 'Latency';

  @override
  String get hostOffline => 'Host offline; showing its last known state';

  @override
  String get retry => 'Retry';

  @override
  String get retryNote => 'Preview is not connected to a real host';

  @override
  String get pairTitle => 'Connect a host';

  @override
  String get pairDescription =>
      'Enter the 6-digit pairing code shown on the host';

  @override
  String get pairCode => 'Pairing code';

  @override
  String get pairHint => 'Pairing code expires in 60 seconds';

  @override
  String get pairDemo => 'Simulate connection';

  @override
  String get pairSuccess => 'Sample host added';

  @override
  String get pairInvalid => 'Enter 6 digits';

  @override
  String get workspace => 'Workspace';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Choose host';

  @override
  String get selectProject => 'Choose project';

  @override
  String get selectBranch => 'Choose worktree';

  @override
  String get mainBranch => 'Main worktree';

  @override
  String get featureBranch => 'Login layout';

  @override
  String get connectionTools => 'Connections';

  @override
  String get workspaceResources => 'Workspace';

  @override
  String get confirm => 'Confirm';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Changes, branches and history';

  @override
  String get gitBranches => 'Branches';

  @override
  String get gitHistory => 'History';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added lines added · $removed removed';
  }

  @override
  String get gitActions => 'Git actions';

  @override
  String get gitFetch => 'Fetch';

  @override
  String get gitPull => 'Pull';

  @override
  String get gitPush => 'Push';

  @override
  String get gitCurrent => 'Current branch';

  @override
  String get gitCreateBranch => 'New branch';

  @override
  String get gitBranchName => 'Branch name';

  @override
  String get gitSwitch => 'Switch branch';

  @override
  String get gitMerge => 'Merge branch';

  @override
  String get gitDeleteBranch => 'Delete branch';

  @override
  String get gitHistoryLayout => 'Adjust login form spacing';

  @override
  String get gitHistoryInit => 'Initialize login page';

  @override
  String get gitPreview => 'Git simulation only; repository unchanged';

  @override
  String get gitDirty => 'Commit current changes first';

  @override
  String get gitSwitchNote => 'Switch branch in this worktree; simulation only';

  @override
  String get gitDeleteNote => 'Delete selected branch; simulation only';

  @override
  String get gitInvalidBranch => 'Invalid name or branch already exists';

  @override
  String get review => 'Review';

  @override
  String get browseFiles => 'Browse worktree';

  @override
  String get reviewFiles => 'View code changes';

  @override
  String get selectWorkspace => 'Project and worktree';

  @override
  String get resourceSummary => '2 chats · 1 terminal';

  @override
  String get searchFiles => 'Search files';

  @override
  String get recentFiles => 'Files';

  @override
  String get src => 'Source';

  @override
  String get folder => 'Folder';

  @override
  String get modified => 'Modified';

  @override
  String get filePreview => 'File preview';

  @override
  String get fileSample => 'Sample file content';

  @override
  String get edit => 'Edit';

  @override
  String get savePreview => 'Changes saved in this preview';

  @override
  String get unsaved => 'Unsaved';

  @override
  String get discard => 'Discard changes';

  @override
  String get discardConfirm => 'Discard unsaved changes to this file?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'More resources';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'Database';

  @override
  String get ports => 'Port forwarding';

  @override
  String get browser => 'Web preview';

  @override
  String get portsSub => '1 forward';

  @override
  String get connectionOwner => 'Execution Node · Studio';

  @override
  String get openTerminal => 'Open terminal';

  @override
  String get tables => 'Tables';

  @override
  String get portNote => 'Sample forward · No local port listener';

  @override
  String get portTarget => 'Target port';

  @override
  String get localPort => 'Local port';

  @override
  String get closePort => 'Close forward';

  @override
  String get portClosed => 'Sample forward closed';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Working tree';

  @override
  String get staged => 'Staged';

  @override
  String get diffSummary => '3 files';

  @override
  String get stage => 'Stage all';

  @override
  String get unstage => 'Unstage';

  @override
  String get commit => 'Commit';

  @override
  String get commitTitle => 'Commit changes';

  @override
  String get commitMessage => 'Commit message';

  @override
  String get commitPlaceholder => 'Describe the changes';

  @override
  String get commitPreview => 'Simulate commit';

  @override
  String get committed => 'Sample commit completed';

  @override
  String get noChanges => 'No changes to commit';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Continue editing';

  @override
  String get diffSelection => 'Choose changed file';

  @override
  String get terminalKeyboard => 'Keyboard';

  @override
  String get terminalEnter => 'Enter';

  @override
  String get terminalOutputLabel => 'Terminal output';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Read only';

  @override
  String get takeControl => 'Take control';

  @override
  String get hasControl => 'Input control';

  @override
  String get releaseControl => 'Release control';

  @override
  String get terminalPlaceholder => 'Enter a sample command';

  @override
  String get terminalPreview => 'Sample terminal · Commands are not executed';

  @override
  String get terminalOutput => 'Command received in preview, not executed';

  @override
  String get terminalControlNote =>
      'Take control to send input; simulated here';

  @override
  String get usageSubtitle => 'Sailry chats only';

  @override
  String get week => 'This week';

  @override
  String get month => 'This month';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total responses priced';
  }

  @override
  String get usageEmpty => 'No usage data';

  @override
  String get estimatedCost => 'Estimated cost';

  @override
  String get costCoverage => '42 / 48 responses priced';

  @override
  String get partial => 'Partial data';

  @override
  String get sourcesPartial => '2 / 3 hosts updated';

  @override
  String get input => 'Input';

  @override
  String get output => 'Output';

  @override
  String get cached => 'Cache hits';

  @override
  String get modelUsage => 'Model distribution';

  @override
  String get hostUsage => 'Host usage';

  @override
  String get recentRequests => 'Recent responses';

  @override
  String get allUsage => 'Usage details';

  @override
  String get usageNote => 'Costs are estimates; some responses are not priced';

  @override
  String get sourceNote => 'Offline hosts retain their last known data';

  @override
  String get profileSubtitle => 'Mobile controller';

  @override
  String get localSettings => 'Local preferences';

  @override
  String get nodeSettings => 'Execution Node settings';

  @override
  String get appearance => 'Appearance';

  @override
  String get notifications => 'Notifications';

  @override
  String get enabled => 'On';

  @override
  String get disabled => 'Off';

  @override
  String get add => 'Add';

  @override
  String get configName => 'Name';

  @override
  String get configEndpoint => 'Endpoint';

  @override
  String get configModels => 'Models';

  @override
  String get configInstructions => 'Instructions';

  @override
  String get configContent => 'Content';

  @override
  String get configEmpty => 'No entries';

  @override
  String get configDuplicate => 'Name already exists';

  @override
  String configDelete(String name) {
    return 'Delete “$name”?';
  }

  @override
  String get speechInput => 'Voice input';

  @override
  String get developerInstructions =>
      'Change the code for the task and verify the result';

  @override
  String get reviewerInstructions => 'Review code changes and identify issues';

  @override
  String get projectConventions => 'Project conventions';

  @override
  String get memoryContent => 'Preserve the existing code style';

  @override
  String get providers => 'Models and providers';

  @override
  String get roles => 'Roles';

  @override
  String get memorySettings => 'Memory';

  @override
  String get speech => 'Speech';

  @override
  String nodeSettingsNote(String host) {
    return 'Configuration saved on $host';
  }

  @override
  String get about => 'About Sailry';

  @override
  String get aboutBody =>
      'Mobile interaction preview, not connected to services';

  @override
  String get settingsSaved => 'Settings updated in this preview';

  @override
  String get modelPicker => 'Choose model';

  @override
  String get nodeDefaults => 'Node defaults';

  @override
  String get providerNote =>
      'Sample configuration · Credentials stay on the execution Node';

  @override
  String get roleNote => 'Sample role · Applies to new chats';

  @override
  String get auto => 'Auto';

  @override
  String get manual => 'Ask each time';

  @override
  String get notificationsNote => 'Controls preview notifications only';

  @override
  String get memoryNote => 'Sample Node memory';

  @override
  String get speechNote => 'Uses the execution Node speech configuration';

  @override
  String get newTaskHost => 'Execution host';

  @override
  String get newTaskProject => 'Project';

  @override
  String get newTaskWorktree => 'Worktree';

  @override
  String get create => 'Create';

  @override
  String get taskCreated => 'Sample chat created';

  @override
  String get required => 'Describe your task first';

  @override
  String get notificationsEmpty => 'No new notifications';

  @override
  String get reviewTitle => 'Design references';

  @override
  String get reviewIntro =>
      'Pages follow the current source; this preview does not establish mobile service acceptance';

  @override
  String get reviewConversation => 'Chats, approvals, questions and queue';

  @override
  String get reviewConversationText =>
      'Tasks retain host, project and worktree ownership; expand tool records and approvals within chats';

  @override
  String get reviewResources => 'Files, Git, terminals and connections';

  @override
  String get reviewResourcesText =>
      'File editing, staging, commits and ports retain their entry points; details open on secondary pages';

  @override
  String get reviewHosts => 'Host connections and monitoring';

  @override
  String get reviewHostsText =>
      'Paired Nodes, 6-digit codes, resource usage and processes; offline state is not shown as live';

  @override
  String get reviewUsage => 'Usage and Node settings';

  @override
  String get reviewUsageText =>
      'Sailry chats only; aggregated usage retains completeness and costs indicate estimates and coverage';

  @override
  String get reviewBoundary => 'Mobile boundary';

  @override
  String get reviewBoundaryText =>
      'The mobile bridge exposes connections, chats, terminals and usage; this preview starts no Node, models, pairing, terminals or plugins';

  @override
  String get reviewVisual => 'Visual references';

  @override
  String get reviewVisualText =>
      'Reference 1: chat hierarchy; reference 2: soft cards and floating navigation; reference 3: compact monitoring';

  @override
  String get hostConnectPrompt => 'Connect a host';

  @override
  String get hostDisconnected => 'Connection lost';

  @override
  String get language => 'Language';

  @override
  String get languageSystem => 'System';

  @override
  String get languageChinese => '中文';

  @override
  String get languageEnglish => 'English';

  @override
  String get backgroundConnection => 'Keep connected in background';

  @override
  String get backgroundConnectionActive => 'Keeping host connections active';

  @override
  String get backgroundConnectionFailed =>
      'Background connection not enabled; retry';

  @override
  String get resetReasoning => 'Reset effort';

  @override
  String get completionAlerts => 'Completion alerts';

  @override
  String get notificationsReadAll => 'Mark all read';

  @override
  String get notificationsOpen => 'Open';

  @override
  String get preferencesFailed => 'Preferences not saved; retry';

  @override
  String get connectFirst => 'Connect a host to begin';

  @override
  String get initializing => 'Starting';

  @override
  String get startupFailed => 'Startup failed';

  @override
  String get retryConnection => 'Retry';

  @override
  String get pairAction => 'Connect';

  @override
  String get pairFailed => 'Connection failed; retry';

  @override
  String get pairExpired => 'Pairing code expired; get a new code';

  @override
  String get pairing => 'Connecting';

  @override
  String get hostUnavailable => 'Host not connected';

  @override
  String get hostMetricsFailed => 'Cannot read host status';

  @override
  String get hostProcessesEmpty => 'No processes';

  @override
  String get hostRegisterProject => 'Add project';

  @override
  String get hostChooseDirectory => 'Choose directory';

  @override
  String get hostChooseFile => 'Choose file';

  @override
  String get hostParentDirectory => 'Parent directory';

  @override
  String get hostEmptyDirectory => 'Directory is empty';

  @override
  String get hostLoadMore => 'Load more';

  @override
  String get hostProjectName => 'Project name';

  @override
  String get hostProjectPath => 'Project path on host';

  @override
  String get hostProjectFailed => 'Cannot add project';

  @override
  String get hostUnknown => 'No data';

  @override
  String get hostRefresh => 'Refresh';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Memory';

  @override
  String get hostMetricDisk => 'Disk';

  @override
  String get failureConflict => 'Content changed; reload and retry';

  @override
  String get failureUnknown =>
      'Outcome unconfirmed; check the host state first';

  @override
  String get failureDenied => 'Permission denied';

  @override
  String get failureUnavailable => 'Host not connected';

  @override
  String get failureBusy => 'Service busy; retry later';

  @override
  String get failureGeneric => 'Operation failed';

  @override
  String get settingsSpeechLanguage => 'Language';

  @override
  String get settingsSpeechAuto => 'Auto-detect';

  @override
  String get settingsSpeechChinese => 'Chinese';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Speech model ready';

  @override
  String get settingsSpeechDownload => 'Download speech model';

  @override
  String get settingsSpeechFailed => 'Speech model not ready; retry';

  @override
  String get settingsNoHost => 'Connect a host first';

  @override
  String get settingsUnavailable => 'Unavailable';

  @override
  String get settingsLoadFailed => 'Could not load';

  @override
  String get settingsSaveFailed => 'Save failed; draft kept';

  @override
  String get settingsConflict => 'Settings changed; reopen and retry';

  @override
  String get settingsUnknown => 'Outcome unconfirmed; refresh to check';

  @override
  String get settingsRetry => 'Retry';

  @override
  String get settingsLoading => 'Loading';

  @override
  String get settingsRequired => 'Enter a value';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => 'Description';

  @override
  String get settingsInstructions => 'Instructions';

  @override
  String get settingsModels => 'Model IDs, one per line';

  @override
  String get settingsApi => 'API format';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API Key';

  @override
  String get settingsEnabled => 'Enabled';

  @override
  String get settingsDriver => 'Service';

  @override
  String get settingsModel => 'Model';

  @override
  String get settingsMemoryAuto => 'Record automatically';

  @override
  String get settingsMemoryBudget => 'Context bytes';

  @override
  String get settingsMemoryReview => 'Review interval (days)';

  @override
  String get settingsMemoryRecords => 'Memory entries';

  @override
  String get settingsMemoryKind => 'Type';

  @override
  String get settingsMemoryUser => 'User';

  @override
  String get settingsMemoryFeedback => 'Feedback';

  @override
  String get settingsMemoryProject => 'Project';

  @override
  String get settingsMemoryReference => 'Reference';

  @override
  String get settingsArchived => 'Archived';

  @override
  String get settingsEmpty => 'No records';

  @override
  String get settingsUsageUnknown => 'Unknown';

  @override
  String get settingsUsagePartial => 'Some hosts are unavailable';

  @override
  String get settingsUsageCache => 'Cached';

  @override
  String get settingsUsageInput => 'Uncached input';

  @override
  String get settingsUsageOutput => 'Output';

  @override
  String get settingsUsageDaily => 'Daily';

  @override
  String get settingsUsageWeekly => 'Weekly';

  @override
  String get settingsUtc => 'UTC';
}
