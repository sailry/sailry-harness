// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Japanese (`ja`).
class AppLocalizationsJa extends AppLocalizations {
  AppLocalizationsJa([String locale = 'ja']) : super(locale);

  @override
  String get updatesVersion => 'バージョン';

  @override
  String get updatesCheck => '更新をチェック';

  @override
  String get updatesChecking => '確認中';

  @override
  String updatesAvailable(String version) {
    return 'バージョン$versionが利用可能です';
  }

  @override
  String get updatesDownload => 'アップデートをダウンロード';

  @override
  String get updatesCurrent => 'あなたは最新の';

  @override
  String get updatesUnpublished => 'モバイル版はまだありません';

  @override
  String get updatesCheckFailed => '更新をチェックできませんでした';

  @override
  String get updatesOpenFailed => 'ダウンロードを開けませんでした';

  @override
  String get welcomeTitle => '今日は何に取り組みたいですか。';

  @override
  String get welcomeExplore => 'プロジェクトを探索';

  @override
  String get welcomeExploreDetail => '構造と入り口を理解する';

  @override
  String get welcomeExplorePrompt => 'このプロジェクトを理解してくれ 重要なモジュールと入り口を含めて';

  @override
  String get welcomeBuild => 'アイデアを構築';

  @override
  String get welcomeBuildDetail => 'アイデアを実現する';

  @override
  String get welcomeBuildPrompt =>
      'このプロジェクトに機能を追加したい。まず必要条件を確認し、実装計画を概説してください。';

  @override
  String get welcomeReview => '変更を見直す';

  @override
  String get welcomeReviewDetail => '変更と潜在的な問題をチェック';

  @override
  String get welcomeReviewPrompt =>
      'このプロジェクトの現在の変化を，潜在的な問題と欠落したテストに焦点を当ててレビューした。';

  @override
  String get welcomePlan => '計画を立てろ';

  @override
  String get welcomePlanDetail => '目標とステップを明確にする';

  @override
  String get welcomePlanPrompt => '次の開発作業の 段階的な計画を作る手伝いを';

  @override
  String get conversationEmpty => 'あなたのタスクを説明';

  @override
  String get conversationLoading => 'チャットを読み込み中';

  @override
  String get conversationReconnecting => '再接続中';

  @override
  String get conversationErrorDetails => '理由を表示';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'モデル要求 $attempt/$limit を再試行中';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'モデルリクエスト再試行 $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count ツールコール';
  }

  @override
  String get conversationGoal => '目標';

  @override
  String get conversationGoalBlocked => 'ブロック';

  @override
  String conversationGoalBudget(String count) {
    return '予算$countトークン';
  }

  @override
  String get conversationStepSkipped => 'スキップ済み';

  @override
  String get conversationChild => 'サブタスク';

  @override
  String get conversationChildReadonly => 'サブタスクチャット';

  @override
  String get conversationOffline => '接続が途絶えました';

  @override
  String get conversationUnavailable => 'チャットが利用できません';

  @override
  String get conversationFailed => '操作に失敗しました。やり直します';

  @override
  String get conversationUnknown => '結果は確認されていません。もう一度行動する前にチャットを確認してください';

  @override
  String get conversationCheckResult => 'チェック結果';

  @override
  String get conversationConflict => '設定が変更されました。再開する前に行動してください';

  @override
  String get conversationOlder => '古いメッセージを読み込む';

  @override
  String get conversationNew => '新しいチャット';

  @override
  String get conversationNoHost => 'まずホストに接続';

  @override
  String get conversationNoProject => 'まずホスト上のプロジェクトを追加';

  @override
  String get conversationNoModel => 'まずホスト上でモデルを設定します';

  @override
  String get conversationNoTasks => 'まだチャットはありません';

  @override
  String get conversationNoMessages => 'メッセージなし';

  @override
  String get conversationPreviewUnavailable => 'メッセージは利用できません';

  @override
  String get conversationInterrupted => '中断済み';

  @override
  String get conversationContinued => '継続済み';

  @override
  String get conversationFailedStatus => '失敗';

  @override
  String get conversationStopping => '停止中';

  @override
  String get conversationQueued => 'キュー';

  @override
  String get conversationProcessing => '処理';

  @override
  String get conversationUnsynced => '同期されていない状態';

  @override
  String get conversationGenerating => '応答';

  @override
  String get conversationWaiting => '確認待ち';

  @override
  String get conversationCompacting => 'コンパクト化文脈';

  @override
  String get conversationForkConfirm => 'このレコードからチャットをフォークする？';

  @override
  String get conversationCompacted => 'コンテキスト圧縮';

  @override
  String get conversationToolWaiting => '待機中';

  @override
  String get conversationToolRunning => '実行中';

  @override
  String get conversationToolReturned => '応答済み';

  @override
  String get conversationToolCancelled => 'キャンセル済み';

  @override
  String get conversationToolNotExecuted => '実行されていません';

  @override
  String get conversationToolInterrupted => '中断済み';

  @override
  String get conversationUnsupportedInput => 'この入力をデスクトップで扱う';

  @override
  String get conversationStartCoding => '実行を開始';

  @override
  String get conversationPlanFeedback => '変更を提案';

  @override
  String get conversationOther => 'その他';

  @override
  String get conversationSubmit => '送信';

  @override
  String get conversationSource => 'ソース';

  @override
  String get conversationMode => 'ワークモード';

  @override
  String get conversationCode => '実行';

  @override
  String get conversationPlan => '計画';

  @override
  String get conversationPermission => '権限';

  @override
  String get conversationAsk => '毎回尋ねる';

  @override
  String get conversationProject => 'プロジェクトアクセス';

  @override
  String get conversationFull => 'フルアクセス';

  @override
  String get conversationReasoning => '推論努力';

  @override
  String get conversationDefault => 'デフォルト';

  @override
  String get conversationNone => 'オフ';

  @override
  String get conversationMinimal => '最小';

  @override
  String get conversationLow => '低';

  @override
  String get conversationMedium => '中';

  @override
  String get conversationHigh => '高';

  @override
  String get conversationXHigh => 'ハイアー';

  @override
  String get conversationMax => '最大';

  @override
  String get conversationBudget => '推論予算';

  @override
  String get conversationAttachment => '添付ファイル';

  @override
  String get conversationAttachmentTooLarge => '添付ファイルが読み込めないか、64MB 以上です';

  @override
  String get conversationDownload => '添付ファイルを表示';

  @override
  String get conversationImageFailed => '画像を表示できません';

  @override
  String get conversationDownloadFailed => '添付ファイルを読み込めません';

  @override
  String get conversationReadonly => 'このチャットはアーカイブされています';

  @override
  String get conversationMicrophoneDenied => 'マイクにアクセスできません';

  @override
  String get conversationRecordingFailed => '音声認識に失敗しました';

  @override
  String get conversationSpeechDisabled => '音声入力がオフ';

  @override
  String get conversationSpeechMissing => 'まず設定で音声モデルをダウンロード';

  @override
  String get conversationRecording => 'レコーディング';

  @override
  String get conversationTranscribing => '転写';

  @override
  String get conversationRecordReady => 'レコーディング準備完了';

  @override
  String get conversationStartRecording => '録音を開始';

  @override
  String get conversationFinishRecording => '録音を終了';

  @override
  String get conversationSources => '出典';

  @override
  String get conversationSearchSuggestions => '検索提案';

  @override
  String get conversationStats => 'チャットの利用';

  @override
  String get conversationStatsEmpty => 'まだ使用していません';

  @override
  String get conversationStatsOverview => '概要';

  @override
  String get conversationStatsTokenGroup => 'トークン使用';

  @override
  String get conversationStatsCostGroup => 'コスト';

  @override
  String get conversationStatsGenerationGroup => '世代';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => '入力';

  @override
  String get conversationStatsOutput => '出力';

  @override
  String get conversationStatsCached => 'キャッシュされた入力';

  @override
  String get conversationStatsReasoning => '推論出力';

  @override
  String get conversationStatsCacheRate => 'キャッシュヒット';

  @override
  String get conversationStatsCost => '推定費用';

  @override
  String get conversationStatsCostCoverage => 'コストキャピタル';

  @override
  String get conversationStatsSpeed => '発生速度';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'タイミングコア';

  @override
  String get conversationStatsTurns => 'ターン';

  @override
  String get conversationStatsResponses => 'モデル応答';

  @override
  String get conversationStatsContext => '現在の文脈';

  @override
  String get conversationStatsInputCost => 'インプットコスト';

  @override
  String get conversationStatsOutputCost => '出力コスト';

  @override
  String get conversationStatsCacheReadCost => 'キャッシュ読み込みコスト';

  @override
  String get conversationStatsCacheWriteCost => 'キャッシュ書き込みコスト';

  @override
  String get messageHistoryUpdated => 'チャットを更新しました。元の記録は保存されました';

  @override
  String get turnUndoUnsaved => 'ファイルには未保存の変更があります。まず保存または削除してください';

  @override
  String get codePlain => 'プレーンテキスト';

  @override
  String get toolArguments => '引数';

  @override
  String get toolResult => '結果';

  @override
  String get toolRaw => '原始結果';

  @override
  String get turnChanges => 'ターン変更';

  @override
  String turnChangesCount(String count) {
    return '$countファイル';
  }

  @override
  String get turnUndo => '変更を無効にする';

  @override
  String get turnUndoAll => 'すべて無効にする';

  @override
  String get turnUndoConfirm => 'このターンからファイルの変更を取り消しますか？後の変更との衝突は操作を停止します';

  @override
  String get turnUndoDone => '取り消し済み';

  @override
  String get turnUndoPartial => 'いくつかの変更を取り消しました。残りのファイルをチェックします';

  @override
  String get messageActions => 'メッセージアクション';

  @override
  String get messageEdit => '編集と再生';

  @override
  String get messageEditConfirm => 'このメッセージと次のチャットを置き換えますか？ファイルは戻されません';

  @override
  String get messageRewind => 'ここでリワード';

  @override
  String get messageRewindConfirm => 'このターンを残して後のメッセージを削除しますか？ファイルは変更されません';

  @override
  String get messageBackup => 'チャットのバックアップを表示';

  @override
  String get messageRegenerate => '再生成';

  @override
  String get messageSearch => '検索チャット';

  @override
  String get messageSearchHint => 'メッセージを検索';

  @override
  String get messageSearchMissing => 'このメッセージはもはや現在のチャットにありません';

  @override
  String get messageSearchStale => 'チャットが変更されました。もう一度検索してください';

  @override
  String get messageNoResults => '一致するメッセージがありません';

  @override
  String get messageCheck => '操作結果をチェック';

  @override
  String get messageReference => 'リファレンス';

  @override
  String get messageReferenceContext => 'この参照はメッセージが送信された時のコンテキストに属します';

  @override
  String get toolFailed => '失敗';

  @override
  String toolExitCode(String code) {
    return '出口コード$code';
  }

  @override
  String toolSignal(String signal) {
    return '信号$signalで終了';
  }

  @override
  String get toolTimedOut => 'コマンドのタイムアウト';

  @override
  String get toolCancelled => 'コマンドキャンセル';

  @override
  String get toolOutcomeUnknown => 'コマンドの結果不明';

  @override
  String get toolQuestionAnswered => '回答済み';

  @override
  String get toolQuestionDeclined => '拒否';

  @override
  String get toolQuestionCancelled => 'キャンセル済み';

  @override
  String get fileLinkUnavailable => 'リンクを開けません';

  @override
  String get imagePreview => '画像のプレビュー';

  @override
  String get fileOpenExternal => '他のアプリケーションで開く';

  @override
  String get fileOpenFailed => 'ファイルを開けません';

  @override
  String get fileNoApplication => 'アプリケーションはこのファイルを開けません';

  @override
  String get fileSaveBeforeShare => '共有する前に変更を保存しますか？';

  @override
  String fileTrashConfirm(String name) {
    return '“$name” をホストのごみ箱に移動しますか？未保存の変更も破棄されます';
  }

  @override
  String get fileTrashUncertain => '削除結果が確認されていません。クエリをやり直します';

  @override
  String get fileSaveFailed => 'ファイルの保存に失敗しました';

  @override
  String get terminalHideKeyboard => 'キーボードを隠す';

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
  String get terminalArrowLeft => '左';

  @override
  String get terminalArrowUp => '上';

  @override
  String get terminalArrowDown => '下';

  @override
  String get terminalArrowRight => '右';

  @override
  String get resourceNoWorkspace => '接続されたホストのワークツリーを選択';

  @override
  String get resourceDisconnected => 'ホストは接続されていません';

  @override
  String get resourceRoot => 'ルート';

  @override
  String get resourceMore => 'もっと読み込む';

  @override
  String get resourcePartial => '部分的なコンテンツを表示';

  @override
  String get resourceEmpty => 'コンテンツなし';

  @override
  String get resourceSaveError => '保存に失敗しました。草稿を保持します';

  @override
  String get resourceReloadConfirm => 'ドラフトを削除して最新のコンテンツを読み込みますか？';

  @override
  String get resourceWorktreeCreate => '新しいワークトリー';

  @override
  String get resourceSessionServices => 'チャットサービス';

  @override
  String get resourceServicesUnavailable => 'サービスリストが利用できません';

  @override
  String get resourceNoServices => 'サービスアドレスが見つかりません';

  @override
  String get resourceServiceOpen => 'オープンサービス';

  @override
  String get resourceRemotePort => 'リモートポート';

  @override
  String get resourceOpenPort => 'フォワードポート';

  @override
  String get resourceOpenBrowser => 'ウェブページのプレビュー';

  @override
  String get resourcePreviewFailed => 'ページを読み込めませんでした';

  @override
  String get resourcePreviewLink => 'このリンクをプレビューで開けません';

  @override
  String get resourceForwardStopped => 'フォーワードを停止';

  @override
  String get resourceTerminalControl => 'コントロールを取る';

  @override
  String get resourceTerminalControlHint => '他のデバイスで制御';

  @override
  String get resourceTerminalClaiming => 'コントロールを取る';

  @override
  String get resourceTerminalReadOnly => '読み取り専用端末';

  @override
  String get resourceTerminalEnded => 'ターミナル終了';

  @override
  String get resourceTerminalConnecting => '接続端子';

  @override
  String get resourceTerminalInput => '端末入力';

  @override
  String get resourceTerminalPaste => '貼り付け';

  @override
  String get resourceGitNotRepository => 'このディレクトリは Git リポジトリではありません';

  @override
  String get resourceInvalidPort => '1-65535 のポートを入力';

  @override
  String get tool_navigate => 'ページを開く';

  @override
  String get tool_back => '戻る';

  @override
  String get tool_forward => '前進';

  @override
  String get tool_refresh => 'ページを更新';

  @override
  String get tool_right_click => 'クリック要素';

  @override
  String get tool_clear => 'テキストを入力';

  @override
  String get tool_select => 'オプションを選択';

  @override
  String get tool_hover => 'ホバー要素';

  @override
  String get tool_scroll => 'スクロールページ';

  @override
  String get tool_press_key => '押すキー';

  @override
  String get tool_new_tab => '新しいタブ';

  @override
  String get tool_list_windows => 'ブラウザタブ';

  @override
  String get tool_switch_window => 'タブを切り替え';

  @override
  String get tool_close_window => 'ウィンドウを閉じる';

  @override
  String get tool_close_session => 'ブラウザを閉じる';

  @override
  String get tool_screenshot => 'キャプチャページ';

  @override
  String get tool_print_to_pdf => 'PDF エクスポート';

  @override
  String get tool_file_upload => 'アップロード';

  @override
  String get tool_downloads => 'ダウンロードを表示';

  @override
  String get tool_save_download => 'ダウンロードしたファイルを保存';

  @override
  String get tool_evaluate_js => 'ページスクリプトを実行';

  @override
  String get tool_get_cookies => 'クッキーを読み込む';

  @override
  String get tool_delete_all_cookies => 'クッキーを変更';

  @override
  String get tool_drag_and_drop => 'ドラッグ要素';

  @override
  String get tool_focus => '焦点要素';

  @override
  String get tool_handle_alert => 'ハンドルページ警告';

  @override
  String get tool_database_catalog => 'データベースをブラウズ';

  @override
  String get tool_database_query => 'データベースのクエリ';

  @override
  String get tool_database_execute => 'データベース操作を実行';

  @override
  String get tool_search_memory => '検索メモリ';

  @override
  String get tool_review_memories => 'レビューメモリ';

  @override
  String get tool_consolidate_memories => 'メモリをマージ';

  @override
  String get tool_save_memory => 'メモリを節約';

  @override
  String get tool_forget_memory => 'メモリを削除';

  @override
  String get tool_update_plan => 'アップデートプラン';

  @override
  String get tool_create_goal => 'ゴールを作成';

  @override
  String get tool_get_goal => '目標を表示';

  @override
  String get tool_update_goal => '更新目標';

  @override
  String get tool_spawn_agent => 'サブエージェント';

  @override
  String get tool_browser_tabs => 'ブラウザタブ';

  @override
  String get tool_browser_read => 'ページを読み込み';

  @override
  String get tool_browser_navigate => 'ページを開く';

  @override
  String get tool_browser_click => 'クリック要素';

  @override
  String get tool_browser_input => 'テキストを入力';

  @override
  String get tool_browser_scroll => 'スクロールページ';

  @override
  String get tool_browser_back => '戻る';

  @override
  String get tool_browser_forward => '前進';

  @override
  String get tool_browser_refresh => 'ページを更新';

  @override
  String get tool_browser_open => '新しいタブ';

  @override
  String get tool_browser_close => 'タブを閉じる';

  @override
  String get tool_browser_focus => 'タブを切り替え';

  @override
  String get tool_browser_select => 'オプションを選択';

  @override
  String get tool_browser_hover => 'ホバー要素';

  @override
  String get tool_browser_key => '押すキー';

  @override
  String get tool_browser_frame => 'スイッチフレーム';

  @override
  String get tool_browser_wait => 'ページを待つ';

  @override
  String get tool_browser_screenshot => 'キャプチャページ';

  @override
  String get tool_ssh_run => 'SSH コマンドを実行';

  @override
  String get tool_ssh_transfer => '転送 SSH ファイル';

  @override
  String get tool_list_worktrees => 'ワークトリーのリスト';

  @override
  String get tool_create_worktree => '新しいワークトリー';

  @override
  String get tool_register_worktree => 'ワークトリーを追加';

  @override
  String get tool_remove_worktree => 'ワークトリーを削除';

  @override
  String get tool_google_search => 'ウェブ検索';

  @override
  String get tool_web_fetch => 'ページを取得';

  @override
  String get tool_fetch_url => 'ページを取得';

  @override
  String get tool_read_file => 'ファイルを読み込む';

  @override
  String get tool_write_file => 'ファイルを書き込む';

  @override
  String get tool_list_directory => 'ディレクトリをブラウズ';

  @override
  String get tool_search_files => 'ファイルを検索';

  @override
  String get tool_run_command => 'コマンドを実行';

  @override
  String get tool_read_command => '背景を表示するコマンド';

  @override
  String get tool_stop_command => '停止コマンド';

  @override
  String get tool_load_skill => 'ロードスキル';

  @override
  String get tool_read_skill_resource => 'スキルリソースを読み込む';

  @override
  String get tool_computer_desktop => 'デスクトップを表示';

  @override
  String get tool_computer_observe => '観察画面';

  @override
  String get tool_computer_input => '制御コンピュータ';

  @override
  String get tool_computer_focus => 'スイッチアプリ';

  @override
  String get tool_computer_open => 'オープンアプリケーション';

  @override
  String get tool_git_status => 'Git 状態';

  @override
  String get tool_git_diff => '差分を表示';

  @override
  String get tool_git_log => 'Gitログ';

  @override
  String get tool_inspect_image => '画像を検査';

  @override
  String get tool_generate_image => 'イメージを生成';

  @override
  String get tool_generate_video => 'ビデオを生成';

  @override
  String get refresh => '更新';

  @override
  String get loading => '読み込み中';

  @override
  String get home => 'チャット';

  @override
  String get idle => '待機中';

  @override
  String get allProjects => '全プロジェクト';

  @override
  String get allWorktrees => 'すべてのワークトリー';

  @override
  String get filterProjects => 'フィルタプロジェクト';

  @override
  String get closeSearch => '検索を閉じる';

  @override
  String onlineHostCount(String count) {
    return '$count ネットワーク';
  }

  @override
  String get taskActions => 'タスクアクション';

  @override
  String get archiveShort => 'ファイル';

  @override
  String get archiveTab => 'アーカイブ';

  @override
  String get archivedTasks => 'アーカイブ';

  @override
  String get delete => '削除';

  @override
  String get deleteTask => 'チャットを削除';

  @override
  String get deleteWarning => 'このチャットは削除後は再開できません';

  @override
  String get busyDelete => 'このチャットを削除する前にタスクを停止';

  @override
  String get stopBeforeDelete => 'タスクを停止';

  @override
  String get restored => '帰国';

  @override
  String get restore => '復元';

  @override
  String get archiveEmpty => 'アーカイブされたチャットはありません';

  @override
  String get archiveKeepsRunning => 'アーカイブは実行中のタスクを停止しません';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'モバイルワークスペース';

  @override
  String get edition => 'モバイル・エクスプロレーション';

  @override
  String get intro => 'タスク、チャット、リモートワークスペース';

  @override
  String get preview => 'プレビュー';

  @override
  String get mixed => 'ライト・アンド・ダーク';

  @override
  String get dark => 'ダーク';

  @override
  String get light => 'ライト';

  @override
  String get gallery => '概要';

  @override
  String get focus => 'シングルスクリーン';

  @override
  String get page => 'ページを選択';

  @override
  String get experience => 'ページを開く';

  @override
  String get backGallery => '概要に戻る';

  @override
  String get design => '特徴・デザイン';

  @override
  String get footer => 'モバイル';

  @override
  String get tasks => 'タスク';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'ホスト';

  @override
  String get resources => 'リソース';

  @override
  String get settings => '設定';

  @override
  String get usage => '使用量';

  @override
  String get changes => '変更';

  @override
  String get terminal => 'ターミナル';

  @override
  String get newTerminal => '新しいターミナル';

  @override
  String get files => 'ファイル';

  @override
  String get project => 'プロジェクト';

  @override
  String get worktree => 'ワークツリー';

  @override
  String get subtitleTasks => 'ホスト間のタスク · 承認と応答が先';

  @override
  String get subtitleChat => '連続チャット · 必要に応じてツール活動を拡張';

  @override
  String get subtitleHosts => '接続、ホストリソース、プロセス';

  @override
  String get subtitleResources => 'ホスト → プロジェクト → ワークトリー';

  @override
  String get subtitleChanges => 'ファイルの差分、ステージング、コミット';

  @override
  String get subtitleTerminal => 'リモート端末・明示的入力制御';

  @override
  String get subtitleUsage => 'Sailry チャット · ホスト間で集計';

  @override
  String get subtitleSettings => 'ローカル設定と実行 Node 設定';

  @override
  String get allHosts => '全ホスト';

  @override
  String get connectedHosts => '2つのオンライン';

  @override
  String get all => 'すべて';

  @override
  String get running => '実行中';

  @override
  String get waiting => '待機中';

  @override
  String get completed => '完了';

  @override
  String get taskProgress => '現在のタスク';

  @override
  String get taskWait => '君の決断を待つ';

  @override
  String get taskRecent => '最近完成した';

  @override
  String get search => '検索';

  @override
  String get searchTasks => 'タスクとプロジェクトを検索';

  @override
  String get filterTasks => 'フィルタタスク';

  @override
  String get noResults => 'マッチするタスクがありません';

  @override
  String get notification => '通知';

  @override
  String get newTask => '新しいタスク';

  @override
  String get newConversation => '新しいチャット';

  @override
  String get question => 'ドキュメンテーションはどの言語を使うべきですか？';

  @override
  String get reply => '返信';

  @override
  String get approval => '承認';

  @override
  String get viewRequest => 'ビュー要求';

  @override
  String get ago => 'さっき';

  @override
  String get minutesAgo => '12分前';

  @override
  String get allow => '一度許可';

  @override
  String get deny => '拒否';

  @override
  String get working => '作業中';

  @override
  String get viewChanges => '変更を表示';

  @override
  String get viewConversation => 'チャットを表示';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '今日 09:36';

  @override
  String get phaseThinking => '思考';

  @override
  String get phaseReading => 'ファイルの読み込み';

  @override
  String get phaseQuestion => '応答待ち';

  @override
  String get phaseEditing => 'ファイルの編集';

  @override
  String get phaseApproval => '承認待ち';

  @override
  String get phaseTesting => '走行試験';

  @override
  String get phaseReply => '応答';

  @override
  String get phaseFollowup => '処理キュー';

  @override
  String get phaseComplete => '完了';

  @override
  String get phaseFailed => 'テスト失敗';

  @override
  String get allowShort => '許可';

  @override
  String get queueShort => 'キュー';

  @override
  String get replyingNow => '応答';

  @override
  String get readFileProgress => 'ファイルを読み込み';

  @override
  String get questionPending => '返事を待っています';

  @override
  String get testTool => 'プロジェクトのテストを実行';

  @override
  String get testFailed => 'テスト失敗';

  @override
  String queueCount(String count) {
    return '$count キュー中のメッセージ';
  }

  @override
  String queuePaused(String count) {
    return 'キューが一時停止しました · $count';
  }

  @override
  String get pauseQueue => 'キューを一時停止';

  @override
  String get resumeQueue => 'キューを再開';

  @override
  String get enqueue => 'キューに追加';

  @override
  String get moveUp => '上へ移動';

  @override
  String get thinkingNow => '思考';

  @override
  String get toolsNow => '実行中';

  @override
  String get thought => '推論';

  @override
  String get toolDiff => 'ファイルの違いを検査';

  @override
  String get approvalResolved => '承認解決';

  @override
  String get chatContinue => 'タスクの説明を続けて';

  @override
  String get describeTask => 'あなたのタスクを説明';

  @override
  String get send => '送信';

  @override
  String get attach => '添付';

  @override
  String get voice => '音声入力';

  @override
  String get attachment => '添付ファイル';

  @override
  String get removeAttachment => '添付ファイルを削除';

  @override
  String get model => 'モデル';

  @override
  String get copy => 'コピー';

  @override
  String get copied => 'コピー';

  @override
  String get copyFailed => 'コピーできません。手動でテキストを選択してください';

  @override
  String get more => 'その他';

  @override
  String get close => '閉じる';

  @override
  String get back => '戻る';

  @override
  String get cancel => 'キャンセル';

  @override
  String get save => '保存';

  @override
  String get queue => 'メッセージキュー';

  @override
  String get queueEmpty => 'キュー中のメッセージはありません';

  @override
  String get fork => '会話を分岐';

  @override
  String get stop => 'タスクを停止';

  @override
  String get stoppedStatus => '停止済み';

  @override
  String get hostSubtitle => '実行ノード';

  @override
  String get pair => 'ホストに接続';

  @override
  String get online => 'オンライン';

  @override
  String get offline => 'オフライン';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'メモリー';

  @override
  String get metrics => 'リソース利用';

  @override
  String get activity => 'アクティビティ';

  @override
  String get processes => 'プロセス';

  @override
  String get network => 'ネットワーク';

  @override
  String get details => '詳細';

  @override
  String get relay => 'リレー';

  @override
  String get retry => '再試行';

  @override
  String get pairTitle => 'ホストに接続';

  @override
  String get pairCode => 'ペアリングコード';

  @override
  String get pairInvalid => '6 桁を入力';

  @override
  String get workspace => 'ワークスペース';

  @override
  String get selectHost => 'ホストを選択';

  @override
  String get selectProject => 'プロジェクトを選択';

  @override
  String get selectBranch => 'ワークツリーを選択';

  @override
  String get mainBranch => 'メインのワークツリー';

  @override
  String get connectionTools => 'コネクション';

  @override
  String get workspaceResources => 'ワークスペース';

  @override
  String get confirm => '確認';

  @override
  String get git => 'Git';

  @override
  String get gitBranches => 'ブランチ';

  @override
  String get gitHistory => '履歴';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added行追加、$removed行削除';
  }

  @override
  String get gitActions => 'Git アクション';

  @override
  String get gitFetch => 'フェッチ';

  @override
  String get gitPull => '引っ張る';

  @override
  String get gitPush => 'プッシュ';

  @override
  String get gitCurrent => '現在の支部';

  @override
  String get gitCreateBranch => '新しいブランチ';

  @override
  String get gitBranchName => '支部名';

  @override
  String get gitSwitch => 'スイッチブランチ';

  @override
  String get gitMerge => 'マージブランチ';

  @override
  String get gitDeleteBranch => 'ブランチを削除';

  @override
  String get gitSwitchNote => 'このワークツリーのブランチを切り替えます。シミュレーションのみ';

  @override
  String get gitDeleteNote => '選択したブランチを削除します。シミュレーションのみ';

  @override
  String get review => 'レビュー';

  @override
  String get reviewFiles => 'コードの変更を表示';

  @override
  String get selectWorkspace => 'プロジェクトとワークトリー';

  @override
  String get searchFiles => 'ファイルを検索';

  @override
  String get recentFiles => 'ファイル';

  @override
  String get src => 'ソース';

  @override
  String get folder => 'フォルダー';

  @override
  String get modified => '変更済み';

  @override
  String get filePreview => 'ファイルのプレビュー';

  @override
  String get edit => '編集';

  @override
  String get unsaved => '未保存';

  @override
  String get discard => '変更を無視';

  @override
  String get connections => 'リソースの追加';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'データベース';

  @override
  String get ports => 'ポートフォワード';

  @override
  String get browser => 'ウェブプレビュー';

  @override
  String get openTerminal => 'ターミナルを開く';

  @override
  String get tables => 'テーブル';

  @override
  String get closePort => 'クローズ・フォーワード';

  @override
  String get workingTree => 'ワーキング・ツリー';

  @override
  String get staged => 'ステージ';

  @override
  String get stage => '全ステージ';

  @override
  String get unstage => 'アンステージ';

  @override
  String get commit => 'コミット';

  @override
  String get commitMessage => 'コミットメッセージ';

  @override
  String get noChanges => 'コミットする変更はありません';

  @override
  String get terminalKeyboard => 'キーボード';

  @override
  String get terminalEnter => 'Enter';

  @override
  String get terminalOutputLabel => '端末出力';

  @override
  String get readOnly => '読み取り専用';

  @override
  String get takeControl => 'コントロールを取る';

  @override
  String get hasControl => '入力制御';

  @override
  String get releaseControl => 'リリース制御';

  @override
  String get usageSubtitle => 'Sailry チャットだけ';

  @override
  String get week => 'この週';

  @override
  String get month => 'この月';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total 応答価格';
  }

  @override
  String get usageEmpty => '使用データなし';

  @override
  String get estimatedCost => '推定費用';

  @override
  String get partial => '部分データ';

  @override
  String get input => '入力';

  @override
  String get output => '出力';

  @override
  String get cached => 'キャッシュヒット';

  @override
  String get modelUsage => 'モデル分布';

  @override
  String get hostUsage => 'ホスト利用率';

  @override
  String get recentRequests => '最近の応答';

  @override
  String get allUsage => '使用例';

  @override
  String get usageNote => 'コストは推定値であり、一部の応答には価格が付けられていません';

  @override
  String get sourceNote => 'オフラインのホストは最後に知られたデータを保持します';

  @override
  String get profileSubtitle => 'モバイルコントローラ';

  @override
  String get localSettings => 'ローカルの設定';

  @override
  String get appearance => '外観';

  @override
  String get notifications => '通知';

  @override
  String get enabled => 'オン';

  @override
  String get disabled => 'オフ';

  @override
  String get add => '追加';

  @override
  String get configName => '名前';

  @override
  String get configEndpoint => 'エンドポイント';

  @override
  String get configModels => 'モデル';

  @override
  String get configInstructions => '指示';

  @override
  String get configContent => 'コンテンツ';

  @override
  String get configEmpty => 'エントリなし';

  @override
  String get configDuplicate => '名前は既に存在します';

  @override
  String configDelete(String name) {
    return '“$name” を削除しますか？';
  }

  @override
  String get speechInput => '音声入力';

  @override
  String get developerInstructions => 'タスクのコードを変更し、結果を確認します';

  @override
  String get reviewerInstructions => 'コードの変更を見直し、問題を特定します';

  @override
  String get projectConventions => 'プロジェクト規約';

  @override
  String get memoryContent => '既存のコードスタイルを保持';

  @override
  String get providers => 'モデルとプロバイダ';

  @override
  String get roles => '役割';

  @override
  String get memorySettings => 'メモリー';

  @override
  String get speech => 'スピーチ';

  @override
  String nodeSettingsNote(String host) {
    return '$host に保存された設定';
  }

  @override
  String get about => 'Sailryについて';

  @override
  String get modelPicker => 'モデルを選択';

  @override
  String get nodeDefaults => 'Nodeデフォルト';

  @override
  String get auto => '自動';

  @override
  String get manual => '毎回尋ねる';

  @override
  String get speechNote => '実行 Node 音声設定を使用';

  @override
  String get required => 'まずタスクを説明してください';

  @override
  String get notificationsEmpty => '新しい通知はありません';

  @override
  String get reviewTitle => '設計参照';

  @override
  String get reviewConversation => 'チャット、承認、質問、キュー';

  @override
  String get reviewConversationText =>
      'タスクはホスト、プロジェクト、ワークトリーの所有権を保持し、チャット内のツールレコードと承認を拡張します';

  @override
  String get reviewResources => 'ファイル、Git、端子と接続';

  @override
  String get reviewResourcesText =>
      'ファイル編集、ステージング、コミット、ポートはエントリポイントを保持します。詳細は二次ページで開きます';

  @override
  String get reviewHosts => 'ホスト接続とモニタリング';

  @override
  String get reviewHostsText =>
      'ペアのノード、6 桁のコード、リソース使用量、プロセス; オフライン状態はライブでは表示されません';

  @override
  String get reviewUsage => '使用量と Node 設定';

  @override
  String get reviewUsageText =>
      'Sailry チャットのみ; 集計された使用量は完全性を保持し、コストは推定値とカバー率を示します';

  @override
  String get reviewBoundary => '移動境界';

  @override
  String get reviewVisual => '視覚的参照';

  @override
  String get reviewVisualText =>
      'リファレンス１：チャット階層，リファレンス２：ソフトカードと浮動ナビゲーション，リファレンス３：コンパクトモニタリング';

  @override
  String get hostConnectPrompt => 'ホストに接続';

  @override
  String get hostDisconnected => '接続が途絶えました';

  @override
  String get language => '言語';

  @override
  String get languageSystem => 'システム';

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
  String get backgroundConnection => 'バックグラウンドで接続を維持';

  @override
  String get backgroundConnectionActive => 'ホスト接続をアクティブに保つ';

  @override
  String get backgroundConnectionFailed => 'バックグラウンド接続を有効にしていません。やり直します';

  @override
  String get resetReasoning => 'リセット努力';

  @override
  String get completionAlerts => '完了警告';

  @override
  String get notificationsReadAll => 'すべてを既読としてマーク';

  @override
  String get notificationsOpen => '開く';

  @override
  String get preferencesFailed => '設定が保存されませんでした。やり直します';

  @override
  String get connectFirst => '開始するホストに接続';

  @override
  String get initializing => '起動中';

  @override
  String get startupFailed => '起動に失敗';

  @override
  String get retryConnection => '再試行';

  @override
  String get pairAction => '接続';

  @override
  String get pairFailed => '接続に失敗しました。やり直します';

  @override
  String get pairExpired => 'ペアリングコードが失効しました。新しいコードを取得してください';

  @override
  String get pairing => '接続中';

  @override
  String get hostUnavailable => 'ホストは接続されていません';

  @override
  String get hostMetricsFailed => 'ホストの状態を読めません';

  @override
  String get hostProcessesEmpty => 'プロセスなし';

  @override
  String get hostRegisterProject => 'プロジェクトを追加';

  @override
  String get hostChooseDirectory => 'ディレクトリを選択';

  @override
  String get hostChooseFile => 'ファイルを選択';

  @override
  String get hostParentDirectory => '親ディレクトリ';

  @override
  String get hostEmptyDirectory => 'ディレクトリは空です';

  @override
  String get hostLoadMore => 'もっと読み込む';

  @override
  String get hostProjectName => 'プロジェクト名';

  @override
  String get hostProjectPath => 'ホスト上のプロジェクトパス';

  @override
  String get hostProjectFailed => 'プロジェクトを追加できません';

  @override
  String get hostUnknown => 'データなし';

  @override
  String get hostRefresh => '更新';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'メモリー';

  @override
  String get hostMetricDisk => 'ディスク';

  @override
  String get failureConflict => 'コンテンツが変更されました。再読み込みして試してください';

  @override
  String get failureUnknown => '結果は確認されていません。まずホストの状態を確認してください';

  @override
  String get failureDenied => '許可を拒否';

  @override
  String get failureUnavailable => 'ホストは接続されていません';

  @override
  String get failureBusy => 'サービスが忙しいので、後でやり直します';

  @override
  String get failureGeneric => '操作に失敗しました';

  @override
  String get settingsSpeechLanguage => '言語';

  @override
  String get settingsSpeechAuto => '自動検出';

  @override
  String get settingsSpeechChinese => '中国語';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => '音声モデル準備完了';

  @override
  String get settingsSpeechDownload => '音声モデルをダウンロード';

  @override
  String get settingsSpeechFailed => '音声モデルが準備ができていません。やり直します';

  @override
  String get settingsNoHost => 'まずホストに接続';

  @override
  String get settingsUnavailable => '利用できません';

  @override
  String get settingsLoadFailed => '読み込めませんでした';

  @override
  String get settingsSaveFailed => '保存に失敗しました。草稿を保持します';

  @override
  String get settingsConflict => '設定が変更されました。再開して試してください';

  @override
  String get settingsUnknown => '結果は確認されていません。チェックするために更新します';

  @override
  String get settingsRetry => '再試行';

  @override
  String get settingsLoading => '読み込み中';

  @override
  String get settingsRequired => '値を入力';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => '説明';

  @override
  String get settingsInstructions => '指示';

  @override
  String get settingsModels => 'モデル ID を行ごとに一つ';

  @override
  String get settingsApi => 'APIフォーマット';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API キー';

  @override
  String get settingsEnabled => '有効';

  @override
  String get settingsDriver => 'サービス';

  @override
  String get settingsModel => 'モデル';

  @override
  String get settingsMemoryAuto => '自動録音';

  @override
  String get settingsMemoryBudget => 'コンテキストバイト';

  @override
  String get settingsMemoryReview => 'レビュー間隔 (日)';

  @override
  String get settingsMemoryRecords => 'メモリエントリ';

  @override
  String get settingsMemoryKind => 'タイプ';

  @override
  String get settingsMemoryUser => 'ユーザー';

  @override
  String get settingsMemoryFeedback => 'フィードバック';

  @override
  String get settingsMemoryProject => 'プロジェクト';

  @override
  String get settingsMemoryReference => 'リファレンス';

  @override
  String get settingsArchived => 'アーカイブ';

  @override
  String get settingsEmpty => '記録なし';

  @override
  String get settingsUsageUnknown => '不明';

  @override
  String get settingsUsagePartial => 'いくつかのホストは利用できません';

  @override
  String get settingsUsageCache => 'キャッシュ';

  @override
  String get settingsUsageInput => '未キャッシュ入力';

  @override
  String get settingsUsageOutput => '出力';

  @override
  String get settingsUsageDaily => 'デイリー';

  @override
  String get settingsUsageWeekly => '週間';

  @override
  String get settingsUtc => 'UTC';

  @override
  String get artifactContents => '目次';

  @override
  String get artifactZoomIn => '拡大';

  @override
  String get artifactZoomOut => '縮小';

  @override
  String get artifactVersions => 'バージョン';

  @override
  String get artifactFormRequired => '必須項目を入力してください';

  @override
  String get artifactFormInvalid => '回答を送信できません';

  @override
  String get artifactRenderFailed => '図を表示できません';

  @override
  String artifactReply(String id, String revision, String title, String form) {
    return '「$title」v$revision（$id/$form）への回答';
  }

  @override
  String get connectionRouteP2p => 'P2P';

  @override
  String get connectionRouteRelay => 'Relay';

  @override
  String get connectionRouteUnknown => '—';

  @override
  String get backgroundNoticeTitle => 'バックグラウンドの結果';

  @override
  String get backgroundCompleted => '完了';

  @override
  String get backgroundFailed => '失敗';

  @override
  String get backgroundCancelled => 'キャンセル済み';

  @override
  String get backgroundStopped => '停止済み';

  @override
  String get backgroundTimedOut => 'タイムアウト';

  @override
  String get backgroundInterrupted => '中断';

  @override
  String get backgroundUnknown => '結果は不確定';

  @override
  String get backgroundPending => 'バックグラウンドの結果を処理できます';
}
