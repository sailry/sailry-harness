// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Korean (`ko`).
class AppLocalizationsKo extends AppLocalizations {
  AppLocalizationsKo([String locale = 'ko']) : super(locale);

  @override
  String get updatesVersion => '버전';

  @override
  String get updatesCheck => '업데이트 확인';

  @override
  String get updatesChecking => '확인 중';

  @override
  String updatesAvailable(String version) {
    return '버전 $version을 사용할 수 있습니다';
  }

  @override
  String get updatesDownload => '업데이트 다운로드';

  @override
  String get updatesCurrent => '당신은 최신 상태입니다';

  @override
  String get updatesUnpublished => '아직 모바일 릴리스 없음';

  @override
  String get updatesCheckFailed => '업데이트를 확인할 수 없음';

  @override
  String get updatesOpenFailed => '다운로드를 열 수 없습니다';

  @override
  String get retryTask => '다시 시도';

  @override
  String get welcomeTitle => '오늘 뭘 하시겠어요?';

  @override
  String get welcomeExplore => '프로젝트 탐색하기';

  @override
  String get welcomeExploreDetail => '구조 및 입력 지점 이해';

  @override
  String get welcomeExplorePrompt =>
      '이 프로젝트를 이해할 수 있도록 도와주세요. 핵심 모듈과 접근 지점을 포함해서요.';

  @override
  String get welcomeBuild => '아이디어 만들기';

  @override
  String get welcomeBuildDetail => '아이디어를 실현하세요';

  @override
  String get welcomeBuildPrompt =>
      '이 프로젝트에 기능을 추가하고 싶습니다. 먼저 요구 사항을 확인하고 구현 계획을 설명해 주십시오.';

  @override
  String get welcomeReview => '변경 사항 검토';

  @override
  String get welcomeReviewDetail => '변경 사항 및 잠재적 문제 확인';

  @override
  String get welcomeReviewPrompt =>
      '잠재적인 문제점과 누락된 테스트에 초점을 맞추어 이 프로젝트의 현재 변경 사항을 검토합니다.';

  @override
  String get welcomePlan => '계획을 세우세요';

  @override
  String get welcomePlanDetail => '목표와 단계 명확화';

  @override
  String get welcomePlanPrompt => '다가오는 개발 작업에 대한 단계별 계획을 작성하는 데 도움을 주십시오.';

  @override
  String get conversationEmpty => '귀하의 작업을 설명';

  @override
  String get conversationLoading => '채팅 불러오는 중';

  @override
  String get conversationReconnecting => '다시 연결 중';

  @override
  String get conversationErrorDetails => '보기 이유';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return '모델 요청을 다시 시도 중 $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return '모델 요청 재시도 $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count 도구 호출';
  }

  @override
  String get conversationGoal => '목표';

  @override
  String get conversationGoalBlocked => '차단됨';

  @override
  String conversationGoalBudget(String count) {
    return '예산 $count 토큰';
  }

  @override
  String get conversationStepSkipped => '건너뜀';

  @override
  String get conversationChild => '하위 작업';

  @override
  String get conversationChildReadonly => '하위 작업 채팅';

  @override
  String get conversationOffline => '연결 끊김Comment';

  @override
  String get conversationUnavailable => '사용할 수 없는 채팅';

  @override
  String get conversationFailed => '작업 실패; 다시 시도하기';

  @override
  String get conversationUnknown => '결과 확인되지 않음; 다시 행동하기 전에 채팅을 확인하십시오';

  @override
  String get conversationCheckResult => '결과 확인';

  @override
  String get conversationConflict => '설정이 변경되었습니다. 다시 실행하기 전에 다시 엽니다';

  @override
  String get conversationOlder => '이전 메시지 불러오기';

  @override
  String get conversationNew => '새 채팅';

  @override
  String get conversationNoHost => '먼저 호스트 연결하기';

  @override
  String get conversationNoProject => '먼저 호스트에 프로젝트 추가';

  @override
  String get conversationNoModel => '먼저 호스트에서 모델을 구성합니다';

  @override
  String get conversationNoTasks => '아직 채팅이 없습니다';

  @override
  String get conversationNoMessages => '메시지 없음';

  @override
  String get conversationPreviewUnavailable => '사용할 수 없는 메시지';

  @override
  String get conversationInterrupted => '중단됨';

  @override
  String get conversationFailedStatus => '실패';

  @override
  String get conversationStopping => '중지 중';

  @override
  String get conversationQueued => '대기열';

  @override
  String get conversationProcessing => '처리 중';

  @override
  String get conversationUnsynced => '동기화되지 않은 상태';

  @override
  String get conversationGenerating => '응답 중';

  @override
  String get conversationWaiting => '확인을 기다리는 중';

  @override
  String get conversationCompacting => '컨텍스트 압축';

  @override
  String get conversationForkConfirm => '이 기록에서 채팅을 포크하시겠습니까?';

  @override
  String get conversationCompacted => '컨텍스트 압축됨';

  @override
  String get conversationToolWaiting => '보류 중';

  @override
  String get conversationToolRunning => '실행 중';

  @override
  String get conversationToolReturned => '응답됨';

  @override
  String get conversationToolCancelled => '취소됨';

  @override
  String get conversationToolNotExecuted => '실행되지 않음';

  @override
  String get conversationToolInterrupted => '중단됨';

  @override
  String get conversationUnsupportedInput => '이 입력을 데스크톱에서 처리하기';

  @override
  String get conversationStartCoding => '실행 시작';

  @override
  String get conversationPlanFeedback => '변경 사항 제안하기';

  @override
  String get conversationOther => '기타';

  @override
  String get conversationSubmit => '제출';

  @override
  String get conversationSource => '소스';

  @override
  String get conversationMode => '작업 모드';

  @override
  String get conversationCode => '실행';

  @override
  String get conversationPlan => '계획';

  @override
  String get conversationPermission => '권한';

  @override
  String get conversationAsk => '매번 물어보기';

  @override
  String get conversationProject => '프로젝트 액세스';

  @override
  String get conversationFull => '전체 액세스';

  @override
  String get conversationReasoning => '추론 노력';

  @override
  String get conversationDefault => '기본값';

  @override
  String get conversationNone => '꺼짐';

  @override
  String get conversationMinimal => '최소';

  @override
  String get conversationLow => '낮음';

  @override
  String get conversationMedium => '중간';

  @override
  String get conversationHigh => '높음';

  @override
  String get conversationXHigh => '더 높음';

  @override
  String get conversationMax => '최대';

  @override
  String get conversationBudget => '예산 추론';

  @override
  String get conversationAttachment => '첨부 파일';

  @override
  String get conversationAttachmentTooLarge => '첨부 파일을 읽을 수 없거나 64 MB 이상 크기';

  @override
  String get conversationDownload => '첨부 파일 보기';

  @override
  String get conversationImageFailed => '이미지를 표시할 수 없음';

  @override
  String get conversationDownloadFailed => '첨부 파일을 불러올 수 없음';

  @override
  String get conversationReadonly => '이 채팅은 보관되었습니다';

  @override
  String get conversationMicrophoneDenied => '마이크에 접근할 수 없음';

  @override
  String get conversationRecordingFailed => '음성 인식 실패Name';

  @override
  String get conversationSpeechDisabled => '음성 입력이 꺼져 있습니다';

  @override
  String get conversationSpeechMissing => '먼저 설정에서 음성 모델 다운로드하기';

  @override
  String get conversationRecording => '녹음 중';

  @override
  String get conversationTranscribing => '번역 중';

  @override
  String get conversationRecordReady => '녹음 준비됨';

  @override
  String get conversationStartRecording => '녹음 시작';

  @override
  String get conversationFinishRecording => '녹음 끝내기';

  @override
  String get conversationSources => '소스';

  @override
  String get conversationSearchSuggestions => '검색 제안';

  @override
  String get conversationStats => '채팅 사용량';

  @override
  String get conversationStatsEmpty => '아직 사용되지 않음';

  @override
  String get conversationStatsOverview => '개요';

  @override
  String get conversationStatsTokenGroup => '토큰 사용';

  @override
  String get conversationStatsCostGroup => '비용';

  @override
  String get conversationStatsGenerationGroup => '생성';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => '입력';

  @override
  String get conversationStatsOutput => '출력';

  @override
  String get conversationStatsCached => '캐시된 입력';

  @override
  String get conversationStatsReasoning => '추론 출력';

  @override
  String get conversationStatsCacheRate => '캐시 히트';

  @override
  String get conversationStatsCost => '예상 비용';

  @override
  String get conversationStatsCostCoverage => '비용 범위';

  @override
  String get conversationStatsSpeed => '생성 속도';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => '타이밍 범위';

  @override
  String get conversationStatsTurns => '턴';

  @override
  String get conversationStatsResponses => '모델 응답';

  @override
  String get conversationStatsContext => '현재 컨텍스트';

  @override
  String get conversationStatsInputCost => '입력 비용';

  @override
  String get conversationStatsOutputCost => '출력 비용';

  @override
  String get conversationStatsCacheReadCost => '캐시 읽기 비용';

  @override
  String get conversationStatsCacheWriteCost => '캐시 쓰기 비용';

  @override
  String get messageHistoryUpdated => '채팅 업데이트; 원본 기록 보관';

  @override
  String get turnUndoUnsaved => '파일에 저장되지 않은 변경 사항이 있습니다. 먼저 저장하거나 버리십시오';

  @override
  String get codePlain => '일반 텍스트';

  @override
  String get toolArguments => '인자';

  @override
  String get toolResult => '결과';

  @override
  String get toolRaw => '원시 결과';

  @override
  String get turnChanges => '변경 사항 돌리기';

  @override
  String turnChangesCount(String count) {
    return '$count 파일';
  }

  @override
  String get turnUndo => '변경 사항 실행 취소';

  @override
  String get turnUndoAll => '모두 실행 취소';

  @override
  String get turnUndoConfirm =>
      '이번 순서에서 파일 변경 사항을 실행 취소하시겠습니까? 나중에 변경한 것과 충돌하면 작업이 중단됩니다';

  @override
  String get turnUndoDone => '실행 취소됨';

  @override
  String get turnUndoPartial => '일부 변경 사항 실행 취소; 나머지 파일 확인';

  @override
  String get messageActions => '메시지 동작';

  @override
  String get messageEdit => '편집 및 재생성';

  @override
  String get messageEditConfirm => '이 메시지와 다음 채팅을 바꾸시겠습니까? 파일은 되돌리지 않습니다';

  @override
  String get messageRewind => '여기로 되감기';

  @override
  String get messageRewindConfirm =>
      '이번 턴으로 돌아가기? 나중에 채팅 기록은 백업됩니다. 파일은 되돌리지 않습니다';

  @override
  String get messageBackup => '채팅 백업 보기';

  @override
  String get messageRegenerate => '다시 생성';

  @override
  String get messageSearch => '채팅 검색';

  @override
  String get messageSearchHint => '메시지 검색';

  @override
  String get messageSearchMissing => '이 메시지는 더 이상 현재 채팅에 없습니다';

  @override
  String get messageSearchStale => '채팅 변경; 다시 검색';

  @override
  String get messageNoResults => '일치하는 메시지가 없음';

  @override
  String get messageCheck => '작업 결과 확인';

  @override
  String get messageReference => '참조';

  @override
  String get messageReferenceContext => '이 참조는 메시지가 전송되었을 때의 컨텍스트에 속합니다';

  @override
  String get toolFailed => '실패';

  @override
  String toolExitCode(String code) {
    return '출구 코드 $code';
  }

  @override
  String toolSignal(String signal) {
    return '신호 $signal에 의해 종료';
  }

  @override
  String get toolTimedOut => '명령 시간 초과';

  @override
  String get toolCancelled => '명령 취소됨';

  @override
  String get toolOutcomeUnknown => '명령 결과 알 수 없음';

  @override
  String get toolQuestionAnswered => '답변됨';

  @override
  String get toolQuestionDeclined => '거절됨';

  @override
  String get toolQuestionCancelled => '취소됨';

  @override
  String get fileLinkUnavailable => '이 링크를 열 수 없습니다';

  @override
  String get imagePreview => '이미지 미리 보기';

  @override
  String get fileOpenExternal => '다른 앱으로 열기';

  @override
  String get fileOpenFailed => '파일을 열 수 없음';

  @override
  String get fileNoApplication => '이 파일을 열 수 있는 앱이 없습니다';

  @override
  String get fileSaveBeforeShare => '공유하기 전에 변경 사항을 저장하시겠습니까?';

  @override
  String fileTrashConfirm(String name) {
    return '“$name”을 호스트 휴지통으로 이동하시겠습니까';
  }

  @override
  String get fileTrashUncertain => '삭제 결과가 확인되지 않았습니다. 쿼리를 다시 시도하십시오';

  @override
  String get fileSaveFailed => '파일 저장 실패';

  @override
  String get terminalHideKeyboard => '키보드 숨기기';

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
  String get terminalArrowLeft => '왼쪽';

  @override
  String get terminalArrowUp => '위';

  @override
  String get terminalArrowDown => '아래';

  @override
  String get terminalArrowRight => '오른쪽';

  @override
  String get resourceNoWorkspace => '연결된 호스트에서 작업 트리 선택';

  @override
  String get resourceDisconnected => '연결되지 않음';

  @override
  String get resourceRoot => '루트';

  @override
  String get resourceMore => '더 많이 로드';

  @override
  String get resourcePartial => '부분적인 내용 표시';

  @override
  String get resourceEmpty => '내용 없음';

  @override
  String get resourceSaveError => '저장 실패; 초안 보관';

  @override
  String get resourceReloadConfirm => '초안을 버리고 최신 콘텐츠를 로드하시겠습니까?';

  @override
  String get resourceWorktreeCreate => '새 작업 트리';

  @override
  String get resourceSessionServices => '채팅 서비스';

  @override
  String get resourceServicesUnavailable => '서비스 목록을 사용할 수 없음';

  @override
  String get resourceNoServices => '서비스 주소가 없습니다';

  @override
  String get resourceServiceOpen => '개방형 서비스';

  @override
  String get resourceRemotePort => '원격 포트';

  @override
  String get resourceOpenPort => '포워드 포트';

  @override
  String get resourceOpenBrowser => '웹 페이지 미리보기';

  @override
  String get resourcePreviewFailed => '페이지를 불러올 수 없습니다';

  @override
  String get resourcePreviewLink => '미리보기에서 이 링크를 열 수 없습니다';

  @override
  String get resourceForwardStopped => '전달 중지됨';

  @override
  String get resourceTerminalControl => '제어하세요';

  @override
  String get resourceTerminalControlHint => '다른 장치에서 제어됨';

  @override
  String get resourceTerminalClaiming => '제어권 획득';

  @override
  String get resourceTerminalReadOnly => '읽기 전용 터미널';

  @override
  String get resourceTerminalEnded => '터미널 종료';

  @override
  String get resourceTerminalConnecting => '터미널 연결 중';

  @override
  String get resourceTerminalInput => '터미널 입력';

  @override
  String get resourceTerminalPaste => '붙여넣기';

  @override
  String get resourceGitNotRepository => '이 디렉토리는 Git 저장소가 아닙니다';

  @override
  String get resourceInvalidPort => '1–65535 사이의 포트를 입력합니다';

  @override
  String get tool_navigate => '페이지 열기';

  @override
  String get tool_back => '뒤로 가기';

  @override
  String get tool_forward => '앞으로 가기';

  @override
  String get tool_refresh => '페이지 새로 고침';

  @override
  String get tool_right_click => '클릭 요소';

  @override
  String get tool_clear => '텍스트 입력';

  @override
  String get tool_select => '옵션 선택';

  @override
  String get tool_hover => '항목 위치 바꾸기';

  @override
  String get tool_scroll => '페이지 스크롤';

  @override
  String get tool_press_key => '키 누르기';

  @override
  String get tool_new_tab => '새 탭';

  @override
  String get tool_list_windows => '브라우저 탭';

  @override
  String get tool_switch_window => '탭 전환';

  @override
  String get tool_close_window => '창 닫기';

  @override
  String get tool_close_session => '브라우저 닫기';

  @override
  String get tool_screenshot => '페이지 캡처';

  @override
  String get tool_print_to_pdf => 'PDF 내보내기';

  @override
  String get tool_file_upload => '파일 업로드';

  @override
  String get tool_downloads => '다운로드 보기';

  @override
  String get tool_save_download => '다운로드한 파일 저장';

  @override
  String get tool_evaluate_js => '페이지 스크립트 실행';

  @override
  String get tool_get_cookies => '쿠키 읽기';

  @override
  String get tool_delete_all_cookies => '쿠키 변경';

  @override
  String get tool_drag_and_drop => '항목 드래그';

  @override
  String get tool_focus => '초점 요소';

  @override
  String get tool_handle_alert => '페이지 알림 처리';

  @override
  String get tool_database_catalog => '데이터베이스 탐색';

  @override
  String get tool_database_query => '데이터베이스 쿼리';

  @override
  String get tool_database_execute => '데이터베이스 작업 실행';

  @override
  String get tool_search_memory => '검색 메모리';

  @override
  String get tool_review_memories => '메모리 검토';

  @override
  String get tool_consolidate_memories => '추억 병합';

  @override
  String get tool_save_memory => '메모리 절약';

  @override
  String get tool_forget_memory => '메모리 삭제';

  @override
  String get tool_update_plan => '계획 업데이트';

  @override
  String get tool_create_goal => '목표 만들기';

  @override
  String get tool_get_goal => '목표 보기';

  @override
  String get tool_update_goal => '업데이트 목표';

  @override
  String get tool_spawn_agent => '하위 에이전트';

  @override
  String get tool_browser_tabs => '브라우저 탭';

  @override
  String get tool_browser_read => '읽기 페이지';

  @override
  String get tool_browser_navigate => '페이지 열기';

  @override
  String get tool_browser_click => '클릭 요소';

  @override
  String get tool_browser_input => '텍스트 입력';

  @override
  String get tool_browser_scroll => '페이지 스크롤';

  @override
  String get tool_browser_back => '뒤로 가기';

  @override
  String get tool_browser_forward => '앞으로 가기';

  @override
  String get tool_browser_refresh => '페이지 새로 고침';

  @override
  String get tool_browser_open => '새 탭';

  @override
  String get tool_browser_close => '탭 닫기';

  @override
  String get tool_browser_focus => '탭 전환';

  @override
  String get tool_browser_select => '옵션 선택';

  @override
  String get tool_browser_hover => '항목 위치 바꾸기';

  @override
  String get tool_browser_key => '키 누르기';

  @override
  String get tool_browser_frame => '프레임 전환';

  @override
  String get tool_browser_wait => '페이지 기다림';

  @override
  String get tool_browser_screenshot => '페이지 캡처';

  @override
  String get tool_ssh_run => 'SSH 명령 실행';

  @override
  String get tool_ssh_transfer => '전송 SSH 파일';

  @override
  String get tool_list_worktrees => '작업 트리 목록';

  @override
  String get tool_create_worktree => '새 작업 트리';

  @override
  String get tool_register_worktree => '작업 트리 추가';

  @override
  String get tool_remove_worktree => '작업 트리 삭제';

  @override
  String get tool_google_search => '웹 검색';

  @override
  String get tool_web_fetch => '페이지 가져오기';

  @override
  String get tool_fetch_url => '페이지 가져오기';

  @override
  String get tool_read_file => '파일 읽기';

  @override
  String get tool_write_file => '파일 쓰기';

  @override
  String get tool_list_directory => '디렉터리 찾아보기';

  @override
  String get tool_search_files => '파일 검색';

  @override
  String get tool_run_command => '명령 실행';

  @override
  String get tool_read_command => '배경 보기 명령';

  @override
  String get tool_stop_command => '중지 명령';

  @override
  String get tool_load_skill => '기술 로드';

  @override
  String get tool_read_skill_resource => '읽기 기술 자원';

  @override
  String get tool_computer_desktop => '데스크톱 보기';

  @override
  String get tool_computer_observe => '화면 관찰';

  @override
  String get tool_computer_input => '제어 컴퓨터';

  @override
  String get tool_computer_focus => '앱 전환';

  @override
  String get tool_computer_open => '앱 열기';

  @override
  String get tool_git_status => 'Git 상태';

  @override
  String get tool_git_diff => '차이 보기';

  @override
  String get tool_git_log => 'Git 로그';

  @override
  String get tool_inspect_image => '이미지 검사';

  @override
  String get tool_generate_image => '이미지 생성';

  @override
  String get tool_generate_video => '비디오 생성';

  @override
  String get terminalUnavailable => '터미널이 연결되어 있지 않음Name';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => '새로고침';

  @override
  String get loading => '로딩 중';

  @override
  String get home => '대화';

  @override
  String get idle => '대기 중';

  @override
  String get allProjects => '모든 프로젝트';

  @override
  String get allWorktrees => '모든 작업 트리';

  @override
  String get filterProjects => '프로젝트 필터';

  @override
  String get closeSearch => '검색 닫기';

  @override
  String onlineHostCount(String count) {
    return '$count 온라인';
  }

  @override
  String get taskActions => '작업 동작';

  @override
  String get archiveShort => '압축 파일';

  @override
  String get archiveTab => '보관함';

  @override
  String get archivedTasks => '보관함';

  @override
  String get delete => '삭제';

  @override
  String get deleteTask => '채팅 삭제';

  @override
  String get deleteWarning => '이 채팅은 삭제 후 재개할 수 없습니다';

  @override
  String get busyDelete => '이 채팅을 삭제하기 전에 작업 중지';

  @override
  String get stopBeforeDelete => '작업 중지';

  @override
  String get deleted => '미리보기에서 채팅이 제거됨';

  @override
  String get restored => '홈으로 복원';

  @override
  String get restore => '복원';

  @override
  String get archiveEmpty => '보관된 채팅 없음';

  @override
  String get archiveKeepsRunning => '아카이빙은 실행 중인 작업을 중단하지 않습니다';

  @override
  String get title => 'Sailry · 모바일 미리보기';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => '모바일 워크스페이스';

  @override
  String get edition => '모바일 탐험 / 01';

  @override
  String get intro => '작업, 채팅 및 원격 워크스페이스';

  @override
  String get preview => '미리 보기';

  @override
  String get sample => '샘플 데이터 · 변경 사항은 이 페이지에 남아 있습니다';

  @override
  String get mixed => '빛과 어둠';

  @override
  String get dark => '다크';

  @override
  String get light => '라이트';

  @override
  String get gallery => '개요';

  @override
  String get focus => '단일 화면';

  @override
  String get reset => '미리 보기 재설정';

  @override
  String get page => '페이지 선택';

  @override
  String get experience => '페이지 열기';

  @override
  String get backGallery => '개요로 돌아가기';

  @override
  String get design => '기능 및 디자인';

  @override
  String get footer => '항해 / 모바일';

  @override
  String get footerNote => '로컬 HTML 미리보기 · 서비스 연결 없음';

  @override
  String get tasks => '작업';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => '호스트';

  @override
  String get resources => '자원';

  @override
  String get settings => '설정';

  @override
  String get usage => '사용량';

  @override
  String get changes => '변경 사항';

  @override
  String get terminal => '터미널';

  @override
  String get newTerminal => '새 터미널';

  @override
  String get files => '파일';

  @override
  String get project => '프로젝트';

  @override
  String get worktree => '작업 트리';

  @override
  String get subtitleTasks => '호스트 간 작업 · 승인과 응답이 먼저';

  @override
  String get subtitleChat => '· 필요에 따라 도구 활동 확장';

  @override
  String get subtitleHosts => '연결, 호스트 리소스 및 프로세스';

  @override
  String get subtitleResources => '호스트 → 프로젝트 → 워크트리';

  @override
  String get subtitleChanges => '파일 차이, 스테이징 및 커밋';

  @override
  String get subtitleTerminal => '원격 터미널 · 명시적 입력 제어';

  @override
  String get subtitleUsage => 'Sailry 채팅 · 호스트 간 집계';

  @override
  String get subtitleSettings => '로컬 환경설정 및 실행 Node 설정';

  @override
  String get allHosts => '모든 호스트';

  @override
  String get connectedHosts => '2 온라인';

  @override
  String get all => '모두';

  @override
  String get running => '실행 중';

  @override
  String get waiting => '보류 중';

  @override
  String get completed => '완료됨';

  @override
  String get taskProgress => '현재 작업';

  @override
  String get taskWait => '귀하의 결정을 기다립니다';

  @override
  String get taskRecent => '최근 완료됨';

  @override
  String get search => '검색';

  @override
  String get searchTasks => '작업 및 프로젝트 검색';

  @override
  String get filterTasks => '작업 필터';

  @override
  String get noResults => '일치하는 작업이 없음';

  @override
  String get notification => '알림';

  @override
  String get newTask => '새 작업';

  @override
  String get newConversation => '새 채팅';

  @override
  String get approveTitle => '로그인 레이아웃 업데이트';

  @override
  String get approveNote => '프로젝트 테스트 실행';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'API 문서 구성';

  @override
  String get questionNote => '응답 기다리는 중';

  @override
  String get question => '어떤 언어로 문서를 작성해야 합니까?';

  @override
  String get optionChinese => '중국어';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => '답장';

  @override
  String get approval => '승인';

  @override
  String get viewRequest => '요청 보기';

  @override
  String get taskSearch => '파일 검색 개선';

  @override
  String get taskSearchNote => '디렉터리 인덱스 확인';

  @override
  String get taskTest => '채팅 복구 수정';

  @override
  String get taskTestNote => '실행 중인 테스트';

  @override
  String get taskDone => '프로젝트 README 업데이트';

  @override
  String get taskDoneNote => '3 파일 변경됨';

  @override
  String get ago => '방금';

  @override
  String get minutesAgo => '12분 전';

  @override
  String get allow => '한 번만 허용';

  @override
  String get deny => '거부';

  @override
  String get approved => '허용 · 샘플';

  @override
  String get denied => '거부됨 · 샘플';

  @override
  String get answered => '답변 · 샘플';

  @override
  String get awaiting => '승인을 기다리는 중';

  @override
  String get working => '작업 중';

  @override
  String get viewChanges => '변경 사항 보기';

  @override
  String get viewConversation => '채팅 보기';

  @override
  String get chatTitle => '로그인 레이아웃 업데이트';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => '오늘 09:36';

  @override
  String get userMessage => '로그인 로직을 유지하면서 로그인 간격을 조정하고 입력 및 버튼 스타일을 통합합니다';

  @override
  String get assistantMessage =>
      '나는 로그인 페이지와 공유 양식 구성 요소, 통합 입력 간격 및 추가 키보드 초점 스타일을 확인';

  @override
  String get replyPreview => '샘플 작업 흐름';

  @override
  String get phaseThinking => '생각';

  @override
  String get phaseReading => '파일 읽기';

  @override
  String get phaseQuestion => '응답 기다리는 중';

  @override
  String get phaseEditing => '파일 편집';

  @override
  String get phaseApproval => '승인을 기다리는 중';

  @override
  String get phaseTesting => '실행 중인 테스트';

  @override
  String get phaseReply => '응답 중';

  @override
  String get phaseFollowup => '대기열 처리 중';

  @override
  String get phaseComplete => '완료됨';

  @override
  String get phaseFailed => '테스트 실패';

  @override
  String get allowShort => '허용';

  @override
  String get queueShort => '대기열';

  @override
  String get confirmShort => '확인';

  @override
  String get todoShort => '할 일';

  @override
  String get todoInspect => '로그인 페이지 검사';

  @override
  String get todoEdit => '양식 스타일 조정';

  @override
  String get todoTest => '프로젝트 테스트 실행';

  @override
  String get todoNarrow => '좁은 화면 간격 확인';

  @override
  String get workProcess => '활동';

  @override
  String workSteps(String count) {
    return '· $count 단계';
  }

  @override
  String get questionRecord => '레이아웃 확인';

  @override
  String get answerRecorded => '답장됨';

  @override
  String get playFlow => '작업 재생';

  @override
  String get pauseFlow => '데모 일시 정지';

  @override
  String get nextFlow => '다음 단계';

  @override
  String get replyingNow => '응답 중';

  @override
  String get toolReadLabel => '읽기';

  @override
  String get toolEditLabel => '편집';

  @override
  String get toolRunLabel => '실행';

  @override
  String get readGroup => '3 줄';

  @override
  String get readFileResult => '파일 읽기';

  @override
  String get readFileProgress => '파일 읽기';

  @override
  String get flowAttachment => '로그인 업데이트: 양식 간격 통합, 키보드 포커스 스타일 추가 및 로그인 논리 보존';

  @override
  String get readResult => 'Login.tsx 및 공유 폼 스타일 읽기\n모바일 버튼 너비가 양식과 다릅니다';

  @override
  String get layoutFindings => '로그인 양식은 데스크톱 간격을 사용하고 모바일 버튼은 컨테이너를 채우지 않습니다';

  @override
  String get layoutQuestion => '모바일 로그인 버튼이 너비를 채워야 하나요?';

  @override
  String get questionPending => '귀하의 답변을 기다리고 있습니다';

  @override
  String get wideButton => '전체 폭 단추 사용하기';

  @override
  String get keepButton => '현재 너비 유지';

  @override
  String get editPlan => '나는 로그인 논리를 보존하고, 간격을 통합하고, 모바일 버튼의 전체 폭을 만들 것입니다';

  @override
  String get editPlanKeep => '버튼 너비와 로그인 논리를 유지하고 간격과 초점 스타일만 조정합니다';

  @override
  String get editThinking => '기존 스타일 변수를 재사용하고 레이아웃 변경을 로그인 양식으로 제한';

  @override
  String get editResult => '업데이트 3 파일\n포커스 스타일 및 모바일 레이아웃 규칙 추가';

  @override
  String get beforeTest => '레이아웃 변경이 완료되었습니다. 다음으로 회귀를 확인하기 위해 프로젝트 테스트를 실행합니다';

  @override
  String get testTool => '프로젝트 테스트 실행';

  @override
  String get testProgress => '로그인 양식 테스트 실행 중\n초점 및 키보드 상호 작용 확인';

  @override
  String get testResult => '12 테스트 통과\n로그인 논리 회귀가 없습니다';

  @override
  String get testFailure =>
      '초점 순서 테스트 실패\n비밀번호 필드에 초점을 맞추기를 기대했지만 사용자 이름 필드에 남아있었습니다';

  @override
  String get testFailed => '테스트 실패';

  @override
  String get flowResult =>
      '로그인 간격 및 포커스 스타일이 통합되고 전체 폭 모바일 버튼이 추가되었습니다. 12개의 테스트 모두 통과하고 로그인 로직은 변경되지 않았습니다';

  @override
  String get queueSample => '좁은 화면에서도 버튼 간격 확인';

  @override
  String queueCount(String count) {
    return '$count 대기 중인 메시지';
  }

  @override
  String queuePaused(String count) {
    return '대기열 일시 정지됨 · $count';
  }

  @override
  String get pauseQueue => '대기열 일시 정지';

  @override
  String get resumeQueue => '대기열 재개';

  @override
  String get sendNext => '다음 보내기';

  @override
  String get enqueue => '대기열에 추가';

  @override
  String get queuedPreview => '샘플 대기열에 추가됨';

  @override
  String get moveUp => '위로 이동';

  @override
  String get followupThinking => '좁은 화면에서 일관된 버튼 간격을 확인하기 위해 기존 중단점 확인';

  @override
  String get followupTool => '좁은 화면 스타일 확인하기';

  @override
  String get followupToolResult => '320px와 390px는 동일한 간격 규칙을 사용합니다';

  @override
  String get followupResult => '좁은 화면 버튼 간격이 일관되며 추가 변경 사항이 필요하지 않음';

  @override
  String get deniedResult => '테스트가 실행되지 않았습니다. 현재 변경 사항은 보존됩니다';

  @override
  String get thinkingNow => '생각';

  @override
  String get toolsNow => '실행 중';

  @override
  String get toolPending => '시작되지 않음';

  @override
  String get thoughtLive => '먼저 로그인 페이지와 양식 구성 요소를 검사하여 간격 및 포커스 변경 사항을 식별합니다';

  @override
  String get toolsShort => '3가지 동작';

  @override
  String get thought => '추론';

  @override
  String get thoughtContent => '기존 양식 구성 요소를 재사용하고 로그인 레이아웃 및 포커스 스타일만 조정';

  @override
  String get toolsComplete => '3개의 작업이 완료됨';

  @override
  String get toolRead => '로그인 및 양식 구성 요소 읽기';

  @override
  String get toolEdit => '간격 및 포커스 스타일 업데이트하기';

  @override
  String get toolDiff => '파일 차이점 검사';

  @override
  String get changedFiles => '3 파일 변경됨';

  @override
  String get approvalBody => 'Studio에서 sailry-web worktree에서 테스트 실행';

  @override
  String get approvalResolved => '승인 해결됨';

  @override
  String get chatContinue => '작업을 계속 설명하십시오';

  @override
  String get describeTask => '귀하의 작업을 설명';

  @override
  String get send => '보내기';

  @override
  String get attach => '첨부';

  @override
  String get voice => '음성 입력';

  @override
  String get voiceNote => '이 미리 보기는 마이크에 접근하지 않습니다';

  @override
  String get attachmentNote => '샘플 첨부 파일 추가';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => '첨부 파일 삭제';

  @override
  String get sentPreview => '미리 보기만 보내지 않음';

  @override
  String get model => '모델';

  @override
  String get modelSource => '현재 채팅 구성 · Studio';

  @override
  String get copy => '복사';

  @override
  String get copied => '복사됨';

  @override
  String get copyFailed => '복사할 수 없습니다. 텍스트를 수동으로 선택하십시오';

  @override
  String get more => '더 보기';

  @override
  String get close => '닫기';

  @override
  String get back => '뒤로';

  @override
  String get cancel => '취소';

  @override
  String get save => '저장';

  @override
  String get select => '선택';

  @override
  String get sessionActions => '채팅 동작';

  @override
  String get queue => '메시지 큐';

  @override
  String get queueEmpty => '대기 중인 메시지 없음';

  @override
  String get fork => '대화 분기';

  @override
  String get forked => '샘플 포크 생성됨';

  @override
  String get archive => '채팅 보관';

  @override
  String get archived => '미리보기로 보관';

  @override
  String get stop => '작업 중지';

  @override
  String get stopped => '미리 보기에서 작업이 중단됨';

  @override
  String get stoppedStatus => '중지됨';

  @override
  String get hostSubtitle => '실행 노드';

  @override
  String get pair => '호스트 연결';

  @override
  String get online => '온라인';

  @override
  String get offline => '오프라인';

  @override
  String get connection => '연결';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 색상';

  @override
  String get laptopSystem => '마지막 접속 시간 2시간 전';

  @override
  String get statusHealthy => '건강';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => '메모리';

  @override
  String get disk => '디스크';

  @override
  String get metrics => '리소스 사용';

  @override
  String get activity => '활동';

  @override
  String get lastHour => '마지막 60분';

  @override
  String get sessionCount => '대화';

  @override
  String get terminalCount => '터미널';

  @override
  String get projectCount => '프로젝트';

  @override
  String get processes => '프로세스';

  @override
  String get process => '이름';

  @override
  String get network => '네트워크';

  @override
  String get details => '상세 정보';

  @override
  String get manageHost => '호스트 세부 정보';

  @override
  String get hostProjects => '호스트 프로젝트';

  @override
  String get connectionDetails => '연결 상세 정보';

  @override
  String get direct => '직접';

  @override
  String get relay => '중계';

  @override
  String get latency => '지연 시간';

  @override
  String get hostOffline => '호스트 오프라인; 알려진 마지막 상태 보이기';

  @override
  String get retry => '다시 시도';

  @override
  String get retryNote => '미리보기가 실제 호스트에 연결되어 있지 않습니다';

  @override
  String get pairTitle => '호스트 연결';

  @override
  String get pairDescription => '호스트에 표시된 6자리 페어링 코드를 입력합니다';

  @override
  String get pairCode => '연결 코드';

  @override
  String get pairHint => '페어링 코드가 60초 후에 만료됩니다';

  @override
  String get pairDemo => '연결 시뮬레이션';

  @override
  String get pairSuccess => '샘플 호스트 추가됨';

  @override
  String get pairInvalid => '6자리를 입력하십시오';

  @override
  String get workspace => '작업 공간';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => '호스트 선택';

  @override
  String get selectProject => '프로젝트 선택';

  @override
  String get selectBranch => '작업 트리 선택';

  @override
  String get mainBranch => '기본 작업 트리';

  @override
  String get featureBranch => '로그인 레이아웃';

  @override
  String get connectionTools => '연결';

  @override
  String get workspaceResources => '작업 공간';

  @override
  String get confirm => '확인';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => '변경사항, 브랜치 및 역사';

  @override
  String get gitBranches => '브랜치';

  @override
  String get gitHistory => '기록';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added 줄 추가 · $removed 줄 제거';
  }

  @override
  String get gitActions => 'Git 작업';

  @override
  String get gitFetch => '가져오기';

  @override
  String get gitPull => '끌어오기';

  @override
  String get gitPush => '밀어 넣기';

  @override
  String get gitCurrent => '현재 브랜치';

  @override
  String get gitCreateBranch => '새 브랜치';

  @override
  String get gitBranchName => '분기 이름';

  @override
  String get gitSwitch => '브랜치 바꾸기';

  @override
  String get gitMerge => '브랜치 병합';

  @override
  String get gitDeleteBranch => '분기 삭제';

  @override
  String get gitHistoryLayout => '로그인 양식 간격 조정';

  @override
  String get gitHistoryInit => '로그인 페이지 시작';

  @override
  String get gitPreview => 'Git 시뮬레이션만; 저장소 변경되지 않음';

  @override
  String get gitDirty => '현재 변경 사항 먼저 커밋하기';

  @override
  String get gitSwitchNote => '이 작업 트리에서 브랜치를 바꿉니다; 시뮬레이션만 가능';

  @override
  String get gitDeleteNote => '선택한 브랜치 삭제; 시뮬레이션만';

  @override
  String get gitInvalidBranch => '잘못된 이름이나 브랜치가 이미 존재합니다';

  @override
  String get review => '검토';

  @override
  String get browseFiles => '작업 트리 탐색';

  @override
  String get reviewFiles => '코드 변경 내용 보기';

  @override
  String get selectWorkspace => '프로젝트 및 작업 트리';

  @override
  String get resourceSummary => '대화 2개 · 터미널 1개';

  @override
  String get searchFiles => '파일 검색';

  @override
  String get recentFiles => '파일';

  @override
  String get src => '소스';

  @override
  String get folder => '폴더';

  @override
  String get modified => '수정됨';

  @override
  String get filePreview => '파일 미리 보기';

  @override
  String get fileSample => '샘플 파일 내용';

  @override
  String get edit => '편집';

  @override
  String get savePreview => '이 미리보기에 저장된 변경 사항';

  @override
  String get unsaved => '저장 안 됨';

  @override
  String get discard => '변경 사항 버리기';

  @override
  String get discardConfirm => '저장되지 않은 이 파일의 변경 사항을 버리시겠습니까?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => '더 많은 리소스';

  @override
  String get ssh => 'SSH';

  @override
  String get database => '데이터베이스';

  @override
  String get ports => '포트 전달';

  @override
  String get browser => '웹 미리 보기';

  @override
  String get portsSub => '앞으로 1';

  @override
  String get connectionOwner => '실행 Node · 스튜디오';

  @override
  String get openTerminal => '터미널 열기';

  @override
  String get tables => '테이블';

  @override
  String get portNote => '샘플 전달 · 로컬 포트 리스너 없음';

  @override
  String get portTarget => '대상 포트';

  @override
  String get localPort => '로컬 포트';

  @override
  String get closePort => '앞으로 닫기';

  @override
  String get portClosed => '샘플 앞으로 닫기';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => '작업 트리';

  @override
  String get staged => '스테이지';

  @override
  String get diffSummary => '3 줄';

  @override
  String get stage => '모든 단계';

  @override
  String get unstage => '무대 탈출';

  @override
  String get commit => '커밋';

  @override
  String get commitTitle => '변경 사항 커밋';

  @override
  String get commitMessage => '커밋 메시지';

  @override
  String get commitPlaceholder => '변경 사항을 설명하십시오';

  @override
  String get commitPreview => '커밋 시뮬레이션';

  @override
  String get committed => '샘플 커밋 완료됨';

  @override
  String get noChanges => '커밋할 변경 사항 없음';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => '편집 계속하기';

  @override
  String get diffSelection => '변경된 파일 선택';

  @override
  String get terminalKeyboard => '키보드';

  @override
  String get terminalEnter => 'Enter';

  @override
  String get terminalOutputLabel => '터미널 출력';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => '읽기 전용';

  @override
  String get takeControl => '제어하세요';

  @override
  String get hasControl => '입력 제어';

  @override
  String get releaseControl => '릴리스 제어';

  @override
  String get terminalPlaceholder => '샘플 명령 입력';

  @override
  String get terminalPreview => '샘플 터미널 · 명령이 실행되지 않음';

  @override
  String get terminalOutput => '미리보기에서 명령을 받았으나 실행되지 않았습니다';

  @override
  String get terminalControlNote => '입력을 보내기 위해 제어를 취합니다. 여기에서 시뮬레이션합니다';

  @override
  String get usageSubtitle => 'Sailry 채팅만';

  @override
  String get week => '이번 주';

  @override
  String get month => '이달';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total 응답 가격';
  }

  @override
  String get usageEmpty => '사용량 데이터 없음';

  @override
  String get estimatedCost => '예상 비용';

  @override
  String get costCoverage => '42 / 48 응답 가격';

  @override
  String get partial => '부분 데이터';

  @override
  String get sourcesPartial => '2 / 3 호스트 업데이트';

  @override
  String get input => '입력';

  @override
  String get output => '출력';

  @override
  String get cached => '캐시 히트';

  @override
  String get modelUsage => '모델 분포';

  @override
  String get hostUsage => '호스트 사용량';

  @override
  String get recentRequests => '최근 응답';

  @override
  String get allUsage => '사용 정보';

  @override
  String get usageNote => '비용은 추정치이며 일부 응답은 가격이 표시되지 않습니다';

  @override
  String get sourceNote => '오프라인 호스트는 마지막으로 알려진 데이터를 유지합니다';

  @override
  String get profileSubtitle => '모바일 컨트롤러';

  @override
  String get localSettings => '로컬 환경설정';

  @override
  String get nodeSettings => '실행 Node 설정';

  @override
  String get appearance => '모양';

  @override
  String get notifications => '알림';

  @override
  String get enabled => '켜짐';

  @override
  String get disabled => '꺼짐';

  @override
  String get add => '추가';

  @override
  String get configName => '이름';

  @override
  String get configEndpoint => '끝점';

  @override
  String get configModels => '모델';

  @override
  String get configInstructions => '지침';

  @override
  String get configContent => '내용';

  @override
  String get configEmpty => '항목 없음';

  @override
  String get configDuplicate => '이름이 이미 존재합니다';

  @override
  String configDelete(String name) {
    return '“$name”을 삭제하시겠습니까?';
  }

  @override
  String get speechInput => '음성 입력';

  @override
  String get developerInstructions => '작업에 대한 코드를 변경하고 결과를 확인';

  @override
  String get reviewerInstructions => '코드 변경 사항 검토 및 문제 식별';

  @override
  String get projectConventions => '프로젝트 컨벤션';

  @override
  String get memoryContent => '기존 코드 스타일 유지';

  @override
  String get providers => '모델과 제공자';

  @override
  String get roles => '역할';

  @override
  String get memorySettings => '메모리';

  @override
  String get speech => '음성';

  @override
  String nodeSettingsNote(String host) {
    return '$host에 저장된 구성';
  }

  @override
  String get about => 'Sailry에 대하여';

  @override
  String get aboutBody => '서비스에 연결되지 않은 모바일 상호 작용 미리보기';

  @override
  String get settingsSaved => '이 미리보기에서 업데이트된 설정';

  @override
  String get modelPicker => '모델 선택';

  @override
  String get nodeDefaults => 'Node 기본값';

  @override
  String get providerNote => '샘플 구성 · 자격 증명이 실행 중에 유지됨 Node';

  @override
  String get roleNote => '샘플 역할 · 새 채팅에 적용';

  @override
  String get auto => '자동';

  @override
  String get manual => '매번 물어보기';

  @override
  String get notificationsNote => '미리 보기 알림만 제어';

  @override
  String get memoryNote => '샘플 Node 메모리';

  @override
  String get speechNote => '실행 Node 음성 구성을 사용합니다';

  @override
  String get newTaskHost => '실행 호스트';

  @override
  String get newTaskProject => '프로젝트';

  @override
  String get newTaskWorktree => '작업 트리';

  @override
  String get create => '만들기';

  @override
  String get taskCreated => '샘플 채팅 생성됨';

  @override
  String get required => '먼저 작업을 설명하십시오';

  @override
  String get notificationsEmpty => '새 알림 없음';

  @override
  String get reviewTitle => '설계 참조 자료';

  @override
  String get reviewIntro => '페이지는 현재 소스를 따릅니다. 이 미리보기는 모바일 서비스 수락을 확립하지 않습니다';

  @override
  String get reviewConversation => '채팅, 승인, 질문 및 대기열';

  @override
  String get reviewConversationText =>
      '작업은 호스트, 프로젝트 및 작업 트리 소유권을 유지하며 채팅 내에서 도구 기록 및 승인을 확장합니다';

  @override
  String get reviewResources => '파일, Git, 터미널 및 연결';

  @override
  String get reviewResourcesText =>
      '파일 편집, 스테이징, 커밋 및 포트는 엔트리 포인트를 유지합니다. 세부 정보는 보조 페이지에서 열립니다';

  @override
  String get reviewHosts => '호스트 연결 및 모니터링';

  @override
  String get reviewHostsText =>
      '페어링된 노드, 6자리 코드, 리소스 사용량 및 프로세스; 오프라인 상태는 라이브로 표시되지 않음';

  @override
  String get reviewUsage => '사용 및 Node 설정';

  @override
  String get reviewUsageText =>
      'Sailry 채팅 전용; 집계된 사용량은 완전성을 유지하고 비용은 추정치 및 범위를 나타냅니다';

  @override
  String get reviewBoundary => '이동 경계';

  @override
  String get reviewBoundaryText =>
      '모바일 브리지는 연결, 채팅, 터미널 및 사용량을 노출합니다. 이 미리보기는 Node, 모델, 페어링, 터미널 또는 플러그인에서 시작됩니다';

  @override
  String get reviewVisual => '시각적 참조';

  @override
  String get reviewVisualText =>
      '참조 1: 채팅 계층; 참조 2: 소프트 카드 및 플로팅 네비게이션; 참조 3: 컴팩트 모니터링';

  @override
  String get hostConnectPrompt => '호스트 연결';

  @override
  String get hostDisconnected => '연결 끊김Comment';

  @override
  String get language => '언어';

  @override
  String get languageSystem => '시스템';

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
  String get backgroundConnection => '백그라운드에서 연결 유지';

  @override
  String get backgroundConnectionActive => '호스트 연결 유지';

  @override
  String get backgroundConnectionFailed => '백그라운드 연결이 활성화되지 않았습니다. 다시 시도하십시오';

  @override
  String get resetReasoning => '노력 초기화';

  @override
  String get completionAlerts => '완료 알림';

  @override
  String get notificationsReadAll => '읽지 않은 편지 선택( C)';

  @override
  String get notificationsOpen => '열기';

  @override
  String get preferencesFailed => '설정이 저장되지 않았습니다. 다시 시도하십시오';

  @override
  String get connectFirst => '시작하려면 호스트 연결';

  @override
  String get initializing => '시작 중';

  @override
  String get startupFailed => '시작 실패';

  @override
  String get retryConnection => '다시 시도';

  @override
  String get pairAction => '연결';

  @override
  String get pairFailed => '연결에 실패했습니다. 다시 시도하십시오';

  @override
  String get pairExpired => '페어링 코드가 만료되었습니다. 새 코드 가져오기';

  @override
  String get pairing => '연결 중';

  @override
  String get hostUnavailable => '연결되지 않음';

  @override
  String get hostMetricsFailed => '호스트 상태를 읽을 수 없음';

  @override
  String get hostProcessesEmpty => '프로세스 없음';

  @override
  String get hostRegisterProject => '프로젝트 추가';

  @override
  String get hostChooseDirectory => '디렉터리 선택';

  @override
  String get hostChooseFile => '파일 선택';

  @override
  String get hostParentDirectory => '상위 디렉터리';

  @override
  String get hostEmptyDirectory => '디렉터리가 비어 있습니다';

  @override
  String get hostLoadMore => '더 많이 로드';

  @override
  String get hostProjectName => '프로젝트 이름';

  @override
  String get hostProjectPath => '호스트의 프로젝트 경로';

  @override
  String get hostProjectFailed => '프로젝트를 추가할 수 없음';

  @override
  String get hostUnknown => '데이터 없음';

  @override
  String get hostRefresh => '새로고침';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => '메모리';

  @override
  String get hostMetricDisk => '디스크';

  @override
  String get failureConflict => '내용이 변경되었습니다. 다시 로드하고 다시 시도하십시오';

  @override
  String get failureUnknown => '결과가 확인되지 않았습니다. 먼저 호스트 상태를 확인하십시오';

  @override
  String get failureDenied => '권한 거부됨';

  @override
  String get failureUnavailable => '연결되지 않음';

  @override
  String get failureBusy => '서비스가 바쁘습니다. 나중에 다시 시도하십시오';

  @override
  String get failureGeneric => '작업 실패';

  @override
  String get settingsSpeechLanguage => '언어';

  @override
  String get settingsSpeechAuto => '자동 감지';

  @override
  String get settingsSpeechChinese => '중국어';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => '음성 모델 준비됨';

  @override
  String get settingsSpeechDownload => '음성 모델 다운로드';

  @override
  String get settingsSpeechFailed => '음성 모델이 준비되지 않았습니다. 다시 시도하십시오';

  @override
  String get settingsNoHost => '먼저 호스트 연결하기';

  @override
  String get settingsUnavailable => '사용할 수 없음';

  @override
  String get settingsLoadFailed => '불러올 수 없음';

  @override
  String get settingsSaveFailed => '저장 실패; 초안 보관';

  @override
  String get settingsConflict => '설정 변경; 다시 열고 다시 시도';

  @override
  String get settingsUnknown => '결과 확인되지 않음; 확인하려면 새로 고침';

  @override
  String get settingsRetry => '다시 시도';

  @override
  String get settingsLoading => '로딩 중';

  @override
  String get settingsRequired => '값 입력';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => '설명';

  @override
  String get settingsInstructions => '지침';

  @override
  String get settingsModels => '모델 ID, 줄당 하나';

  @override
  String get settingsApi => 'API 포맷';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API 키';

  @override
  String get settingsEnabled => '활성화';

  @override
  String get settingsDriver => '서비스';

  @override
  String get settingsModel => '모델';

  @override
  String get settingsMemoryAuto => '자동 녹음';

  @override
  String get settingsMemoryBudget => '컨텍스트 바이트';

  @override
  String get settingsMemoryReview => '검토 간격( 일)';

  @override
  String get settingsMemoryRecords => '메모리 항목';

  @override
  String get settingsMemoryKind => '종류';

  @override
  String get settingsMemoryUser => '사용자';

  @override
  String get settingsMemoryFeedback => '피드백';

  @override
  String get settingsMemoryProject => '프로젝트';

  @override
  String get settingsMemoryReference => '참조';

  @override
  String get settingsArchived => '보관함';

  @override
  String get settingsEmpty => '기록 없음';

  @override
  String get settingsUsageUnknown => '알 수 없음';

  @override
  String get settingsUsagePartial => '일부 호스트를 사용할 수 없습니다';

  @override
  String get settingsUsageCache => '캐시됨';

  @override
  String get settingsUsageInput => '캐시되지 않은 입력';

  @override
  String get settingsUsageOutput => '출력';

  @override
  String get settingsUsageDaily => '매일';

  @override
  String get settingsUsageWeekly => '주간';

  @override
  String get settingsUtc => 'UTC';
}
