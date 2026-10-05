// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Russian (`ru`).
class AppLocalizationsRu extends AppLocalizations {
  AppLocalizationsRu([String locale = 'ru']) : super(locale);

  @override
  String get updatesVersion => 'Версия';

  @override
  String get updatesCheck => 'Проверка на наличие обновлений';

  @override
  String get updatesChecking => 'Проверка';

  @override
  String updatesAvailable(String version) {
    return 'Версия $version доступна';
  }

  @override
  String get updatesDownload => 'Загрузить обновление';

  @override
  String get updatesCurrent => 'Вы в курсе событий';

  @override
  String get updatesUnpublished => 'Мобильного релиза пока нет';

  @override
  String get updatesCheckFailed => 'Не удалось проверить наличие обновлений';

  @override
  String get updatesOpenFailed => 'Не удалось открыть загрузочный файл';

  @override
  String get retryTask => 'Повторить';

  @override
  String get welcomeTitle => 'Над чем бы ты хотела поработать сегодня?';

  @override
  String get welcomeExplore => 'Исследовать проект';

  @override
  String get welcomeExploreDetail => 'Понимание его структуры и входных точек';

  @override
  String get welcomeExplorePrompt =>
      'Помогите мне понять этот проект, включая его ключевые модули и входные точки.';

  @override
  String get welcomeBuild => 'Создайте идею';

  @override
  String get welcomeBuildDetail => 'Воплотите свою идею в жизнь';

  @override
  String get welcomeBuildPrompt =>
      'Я хочу добавить функцию к этому проекту. Сначала подтвердите требования со мной и очертите план реализации.';

  @override
  String get welcomeReview => 'Обзор изменений';

  @override
  String get welcomeReviewDetail =>
      'Проверка изменений и потенциальных проблем';

  @override
  String get welcomeReviewPrompt =>
      'Обзор текущих изменений в этом проекте с уделением особого внимания потенциальным проблемам и отсутствующим тестам.';

  @override
  String get welcomePlan => 'Составьте план';

  @override
  String get welcomePlanDetail => 'Уточнение целей и шагов';

  @override
  String get welcomePlanPrompt =>
      'Помогите мне создать поэтапный план для предстоящей работы по разработке.';

  @override
  String get conversationEmpty => 'Опишите свою задачу';

  @override
  String get conversationLoading => 'Загрузка чата';

  @override
  String get conversationReconnecting => 'Повторное подключение';

  @override
  String get conversationErrorDetails => 'Посмотреть причину';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Повторный запрос модели $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Тип запроса перепробован $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count вызовов инструментов';
  }

  @override
  String get conversationGoal => 'Цель';

  @override
  String get conversationGoalBlocked => 'Заблокировано';

  @override
  String conversationGoalBudget(String count) {
    return 'Бюджет $count жетонов';
  }

  @override
  String get conversationStepSkipped => 'Пропущено';

  @override
  String get conversationChild => 'Подзадача';

  @override
  String get conversationChildReadonly => 'Подзадача чата';

  @override
  String get conversationOffline => 'Соединение прервано';

  @override
  String get conversationUnavailable => 'Чат недоступен';

  @override
  String get conversationFailed => 'Операция не удалась; повторите попытку';

  @override
  String get conversationUnknown =>
      'Результат не подтвержден; проверьте чат, прежде чем действовать снова';

  @override
  String get conversationCheckResult => 'Результат проверки';

  @override
  String get conversationConflict =>
      'Изменена конфигурация; перезапустить, прежде чем действовать вновь';

  @override
  String get conversationOlder => 'Загрузить старые сообщения';

  @override
  String get conversationNew => 'Новый чат';

  @override
  String get conversationNoHost => 'Сначала подключите узел';

  @override
  String get conversationNoProject => 'Сначала добавьте проект на узле';

  @override
  String get conversationNoModel => 'Сначала настройте модель на узле';

  @override
  String get conversationNoTasks => 'Еще нет чатов';

  @override
  String get conversationNoMessages => 'Сообщений нет';

  @override
  String get conversationPreviewUnavailable => 'сообщение недоступно';

  @override
  String get conversationInterrupted => 'Прервано';

  @override
  String get conversationFailedStatus => 'Ошибка';

  @override
  String get conversationStopping => 'Остановка';

  @override
  String get conversationQueued => 'В очереди';

  @override
  String get conversationProcessing => 'Обработка';

  @override
  String get conversationUnsynced => 'Состояние не синхронизировано';

  @override
  String get conversationGenerating => 'Реагирование';

  @override
  String get conversationWaiting => 'Ожидается подтверждение';

  @override
  String get conversationCompacting => 'Сжатие контекста';

  @override
  String get conversationForkConfirm => 'Выделить чат из этой записи?';

  @override
  String get conversationCompacted => 'Контекст уплотнён';

  @override
  String get conversationToolWaiting => 'В ожидании';

  @override
  String get conversationToolRunning => 'Выполняется';

  @override
  String get conversationToolReturned => 'Возвращено';

  @override
  String get conversationToolCancelled => 'Отменено';

  @override
  String get conversationToolNotExecuted => 'Не выполнены';

  @override
  String get conversationToolInterrupted => 'Прервано';

  @override
  String get conversationUnsupportedInput =>
      'Обрабатывать этот ввод на рабочем столе';

  @override
  String get conversationStartCoding => 'Начало исполнения';

  @override
  String get conversationPlanFeedback => 'Предложить изменения';

  @override
  String get conversationOther => 'Другое';

  @override
  String get conversationSubmit => 'Отправить';

  @override
  String get conversationSource => 'Источник';

  @override
  String get conversationMode => 'Режим работы';

  @override
  String get conversationCode => 'Выполнить';

  @override
  String get conversationPlan => 'План';

  @override
  String get conversationPermission => 'Разрешения';

  @override
  String get conversationAsk => 'Задавайте каждый раз';

  @override
  String get conversationProject => 'Доступ к проекту';

  @override
  String get conversationFull => 'Полный доступ';

  @override
  String get conversationReasoning => 'Усилия по размышлению';

  @override
  String get conversationDefault => 'По умолчанию';

  @override
  String get conversationNone => 'Выкл';

  @override
  String get conversationMinimal => 'Минимальный';

  @override
  String get conversationLow => 'Низкий';

  @override
  String get conversationMedium => 'Средний';

  @override
  String get conversationHigh => 'Высокий';

  @override
  String get conversationXHigh => 'Выше';

  @override
  String get conversationMax => 'Максимум';

  @override
  String get conversationBudget => 'Обоснование бюджета';

  @override
  String get conversationAttachment => 'Вложение';

  @override
  String get conversationAttachmentTooLarge =>
      'Вложение нечитаемое или больше 64 МБ';

  @override
  String get conversationDownload => 'Посмотреть приложение';

  @override
  String get conversationImageFailed => 'Не удается отобразить изображение';

  @override
  String get conversationDownloadFailed => 'Не удалось загрузить вложение';

  @override
  String get conversationReadonly => 'Этот чат архивирован';

  @override
  String get conversationMicrophoneDenied =>
      'Не удается получить доступ к микрофону';

  @override
  String get conversationRecordingFailed => 'Распознавание речи не удалось';

  @override
  String get conversationSpeechDisabled => 'Голосовой вход отключен';

  @override
  String get conversationSpeechMissing =>
      'Сначала загрузите модель речи в Настройки';

  @override
  String get conversationRecording => 'Запись';

  @override
  String get conversationTranscribing => 'Транскрипция';

  @override
  String get conversationRecordReady => 'Готов к записи';

  @override
  String get conversationStartRecording => 'Начать запись';

  @override
  String get conversationFinishRecording => 'Завершить запись';

  @override
  String get conversationSources => 'Источники';

  @override
  String get conversationSearchSuggestions => 'Предложения по поиску';

  @override
  String get conversationStats => 'Использование чата';

  @override
  String get conversationStatsEmpty => 'Еще не используется';

  @override
  String get conversationStatsOverview => 'Общий обзор';

  @override
  String get conversationStatsTokenGroup => 'Использование токенов';

  @override
  String get conversationStatsCostGroup => 'Стоимость';

  @override
  String get conversationStatsGenerationGroup => 'Поколение';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Ввод';

  @override
  String get conversationStatsOutput => 'Вывод';

  @override
  String get conversationStatsCached => 'Кэшированный ввод';

  @override
  String get conversationStatsReasoning => 'Обоснование выхода';

  @override
  String get conversationStatsCacheRate => 'Поиски в кэше';

  @override
  String get conversationStatsCost => 'Сметные расходы';

  @override
  String get conversationStatsCostCoverage => 'Покрытие расходов';

  @override
  String get conversationStatsSpeed => 'Скорость генерации';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Временной охват';

  @override
  String get conversationStatsTurns => 'Ходы';

  @override
  String get conversationStatsResponses => 'Типовые ответы';

  @override
  String get conversationStatsContext => 'Нынешний контекст';

  @override
  String get conversationStatsInputCost => 'Стоимость вводимых ресурсов';

  @override
  String get conversationStatsOutputCost => 'Стоимость выпуска';

  @override
  String get conversationStatsCacheReadCost => 'Стоимость чтения кэша';

  @override
  String get conversationStatsCacheWriteCost => 'Стоимость записи в кэш';

  @override
  String get messageHistoryUpdated =>
      'Обновление чата; сохранение первоначальных записей';

  @override
  String get turnUndoUnsaved =>
      'В файлах имеются несохраненные изменения; сначала сохраните или отбросьте их';

  @override
  String get codePlain => 'Обычный текст';

  @override
  String get toolArguments => 'Аргументы';

  @override
  String get toolResult => 'Результат';

  @override
  String get toolRaw => 'Исходный результат';

  @override
  String get turnChanges => 'Изменения поворотов';

  @override
  String turnChangesCount(String count) {
    return '$count файлы';
  }

  @override
  String get turnUndo => 'Отмена изменений';

  @override
  String get turnUndoAll => 'Отменить все';

  @override
  String get turnUndoConfirm =>
      'Отменить эти изменения файлов с этого хода? Конфликты с последующими изменениями остановит операцию';

  @override
  String get turnUndoDone => 'Отменено';

  @override
  String get turnUndoPartial =>
      'Некоторые изменения отменены; проверка оставшихся файлов';

  @override
  String get messageActions => 'Действия по сообщению';

  @override
  String get messageEdit => 'Редактирование и восстановление';

  @override
  String get messageEditConfirm =>
      'Заменить это сообщение и следующий чат? Файлы не будут возвращены';

  @override
  String get messageRewind => 'Перематывать здесь';

  @override
  String get messageRewindConfirm =>
      'Перемотка на этот ход? Позднее записи чата будут сохранены; файлы не будут отброшены';

  @override
  String get messageBackup => 'Просмотр резервного копирования чата';

  @override
  String get messageRegenerate => 'Сгенерировать заново';

  @override
  String get messageSearch => 'Поиск чата';

  @override
  String get messageSearchHint => 'Поиск сообщений';

  @override
  String get messageSearchMissing => 'Это сообщение больше не в текущем чате';

  @override
  String get messageSearchStale => 'Чат изменился; поиск снова';

  @override
  String get messageNoResults => 'Соответствующих сообщений нет';

  @override
  String get messageCheck => 'Проверка результата операции';

  @override
  String get messageReference => 'Справочная информация';

  @override
  String get messageReferenceContext =>
      'Эта ссылка относится к контексту, когда сообщение было отправлено';

  @override
  String get toolFailed => 'Ошибка';

  @override
  String toolExitCode(String code) {
    return 'Код выхода $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Прекращается сигналом $signal';
  }

  @override
  String get toolTimedOut => 'Тайм- аут команды';

  @override
  String get toolCancelled => 'Команда отменена';

  @override
  String get toolOutcomeUnknown => 'Результат командования неизвестен';

  @override
  String get toolQuestionAnswered => 'Ответы';

  @override
  String get toolQuestionDeclined => 'Отклонено';

  @override
  String get toolQuestionCancelled => 'Отменено';

  @override
  String get fileLinkUnavailable => 'Не удается открыть эту ссылку';

  @override
  String get imagePreview => 'Предварительный просмотр изображения';

  @override
  String get fileOpenExternal => 'Открыть в другом приложении';

  @override
  String get fileOpenFailed => 'Не удается открыть файл';

  @override
  String get fileNoApplication =>
      'Никакое приложение не может открыть этот файл';

  @override
  String get fileSaveBeforeShare => 'Сохранить изменения перед обменом?';

  @override
  String fileTrashConfirm(String name) {
    return 'Переместить “$name” в корзину хоста? Несохраненные изменения также будут отброшены';
  }

  @override
  String get fileTrashUncertain =>
      'Результат удаления не подтвержден; повторите запрос';

  @override
  String get fileSaveFailed => 'Сохранение файла не удалось';

  @override
  String get terminalHideKeyboard => 'Скрыть клавиатуру';

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
  String get terminalArrowLeft => 'Слева';

  @override
  String get terminalArrowUp => 'Вверх';

  @override
  String get terminalArrowDown => 'Вниз';

  @override
  String get terminalArrowRight => 'Справа';

  @override
  String get resourceNoWorkspace =>
      'Выбор рабочего дерева на подключенном узле';

  @override
  String get resourceDisconnected => 'Узел не подключен';

  @override
  String get resourceRoot => 'Корень';

  @override
  String get resourceMore => 'Загрузить больше';

  @override
  String get resourcePartial => 'Показано частичное содержимое';

  @override
  String get resourceEmpty => 'Нет контента';

  @override
  String get resourceSaveError => 'Сохранить не удалось; проект сохранен';

  @override
  String get resourceReloadConfirm =>
      'Отбросить черновик и загрузить последний контент?';

  @override
  String get resourceWorktreeCreate => 'Новое рабочее дерево';

  @override
  String get resourceSessionServices => 'Услуги чата';

  @override
  String get resourceServicesUnavailable => 'Список услуг недоступен';

  @override
  String get resourceNoServices => 'Адреса сервисов не найдено';

  @override
  String get resourceServiceOpen => 'Открытая служба';

  @override
  String get resourceRemotePort => 'Удаленный порт';

  @override
  String get resourceOpenPort => 'Передний порт';

  @override
  String get resourceOpenBrowser => 'Предварительный просмотр веб-страницы';

  @override
  String get resourcePreviewFailed => 'Не удалось загрузить страницу';

  @override
  String get resourcePreviewLink =>
      'Не удается открыть эту ссылку в предварительном просмотре';

  @override
  String get resourceForwardStopped => 'Переадресация остановлена';

  @override
  String get resourceTerminalControl => 'Взять под контроль';

  @override
  String get resourceTerminalControlHint => 'Управляется другим устройством';

  @override
  String get resourceTerminalClaiming => 'Взятие под контроль';

  @override
  String get resourceTerminalReadOnly => 'Терминал только для чтения';

  @override
  String get resourceTerminalEnded => 'Терминал закрыт';

  @override
  String get resourceTerminalConnecting => 'Соединительная клемма';

  @override
  String get resourceTerminalInput => 'Вход терминала';

  @override
  String get resourceTerminalPaste => 'Вставить';

  @override
  String get resourceGitNotRepository =>
      'Этот каталог не является хранилищем Git';

  @override
  String get resourceInvalidPort => 'Введите порт из 1–65535';

  @override
  String get tool_navigate => 'Открыть страницу';

  @override
  String get tool_back => 'Вернуться назад';

  @override
  String get tool_forward => 'Вперед';

  @override
  String get tool_refresh => 'Обновить страницу';

  @override
  String get tool_right_click => 'Нажмите элемент';

  @override
  String get tool_clear => 'Введите текст';

  @override
  String get tool_select => 'Выберите вариант';

  @override
  String get tool_hover => 'Ховер';

  @override
  String get tool_scroll => 'Прокрутка страницы';

  @override
  String get tool_press_key => 'Нажатие клавиши';

  @override
  String get tool_new_tab => 'Новая вкладка';

  @override
  String get tool_list_windows => 'Вкладки браузера';

  @override
  String get tool_switch_window => 'Переключатель вкладки';

  @override
  String get tool_close_window => 'Закрыть окно';

  @override
  String get tool_close_session => 'Закрыть браузер';

  @override
  String get tool_screenshot => 'Захват страницы';

  @override
  String get tool_print_to_pdf => 'Экспорт PDF';

  @override
  String get tool_file_upload => 'Загрузить файл';

  @override
  String get tool_downloads => 'Посмотреть загрузки';

  @override
  String get tool_save_download => 'Сохранить загруженный файл';

  @override
  String get tool_evaluate_js => 'Выполнить скрипт страницы';

  @override
  String get tool_get_cookies => 'Чтение файлов cookie';

  @override
  String get tool_delete_all_cookies => 'Изменить файлы cookie';

  @override
  String get tool_drag_and_drop => 'Перетаскивание элемента';

  @override
  String get tool_focus => 'Фокусный элемент';

  @override
  String get tool_handle_alert => 'Предупреждение об обработке страницы';

  @override
  String get tool_database_catalog => 'Просмотр базы данных';

  @override
  String get tool_database_query => 'Запрос базы данных';

  @override
  String get tool_database_execute => 'Выполнить операцию с базой данных';

  @override
  String get tool_search_memory => 'Память поиска';

  @override
  String get tool_review_memories => 'Обзор памяти';

  @override
  String get tool_consolidate_memories => 'Слияние воспоминаний';

  @override
  String get tool_save_memory => 'Экономия памяти';

  @override
  String get tool_forget_memory => 'Удалить память';

  @override
  String get tool_update_plan => 'Обновление плана';

  @override
  String get tool_create_goal => 'Создать цель';

  @override
  String get tool_get_goal => 'Показать цель';

  @override
  String get tool_update_goal => 'Цель обновления';

  @override
  String get tool_spawn_agent => 'Субагент';

  @override
  String get tool_browser_tabs => 'Вкладки браузера';

  @override
  String get tool_browser_read => 'Читать страницу';

  @override
  String get tool_browser_navigate => 'Открыть страницу';

  @override
  String get tool_browser_click => 'Нажмите элемент';

  @override
  String get tool_browser_input => 'Введите текст';

  @override
  String get tool_browser_scroll => 'Прокрутка страницы';

  @override
  String get tool_browser_back => 'Вернуться назад';

  @override
  String get tool_browser_forward => 'Вперед';

  @override
  String get tool_browser_refresh => 'Обновить страницу';

  @override
  String get tool_browser_open => 'Новая вкладка';

  @override
  String get tool_browser_close => 'Закрыть вкладку';

  @override
  String get tool_browser_focus => 'Переключатель вкладки';

  @override
  String get tool_browser_select => 'Выберите вариант';

  @override
  String get tool_browser_hover => 'Ховер';

  @override
  String get tool_browser_key => 'Нажатие клавиши';

  @override
  String get tool_browser_frame => 'Рамка переключателя';

  @override
  String get tool_browser_wait => 'Ожидание страницы';

  @override
  String get tool_browser_screenshot => 'Захват страницы';

  @override
  String get tool_ssh_run => 'Выполнить команду SSH';

  @override
  String get tool_ssh_transfer => 'Перенести файл SSH';

  @override
  String get tool_list_worktrees => 'Список рабочих деревьев';

  @override
  String get tool_create_worktree => 'Новое рабочее дерево';

  @override
  String get tool_register_worktree => 'Добавить рабочее дерево';

  @override
  String get tool_remove_worktree => 'Удалить рабочее дерево';

  @override
  String get tool_google_search => 'Поиск в сети';

  @override
  String get tool_web_fetch => 'Получить страницу';

  @override
  String get tool_fetch_url => 'Получить страницу';

  @override
  String get tool_read_file => 'Читать файл';

  @override
  String get tool_write_file => 'Запись файла';

  @override
  String get tool_list_directory => 'Просмотр каталога';

  @override
  String get tool_search_files => 'Поиск файлов';

  @override
  String get tool_run_command => 'Выполнить команду';

  @override
  String get tool_read_command => 'Команда «Просмотреть фон»';

  @override
  String get tool_stop_command => 'Команда Stop';

  @override
  String get tool_load_skill => 'Навыки нагрузки';

  @override
  String get tool_read_skill_resource => 'Навыки чтения';

  @override
  String get tool_computer_desktop => 'Просмотр рабочего стола';

  @override
  String get tool_computer_observe => 'Экран наблюдения';

  @override
  String get tool_computer_input => 'Компьютер управления';

  @override
  String get tool_computer_focus => 'Приложение Switch';

  @override
  String get tool_computer_open => 'Открыть приложение';

  @override
  String get tool_git_status => 'Git статус';

  @override
  String get tool_git_diff => 'Показать различия';

  @override
  String get tool_git_log => 'Git лог';

  @override
  String get tool_inspect_image => 'Проверить изображение';

  @override
  String get tool_generate_image => 'Создание образа';

  @override
  String get tool_generate_video => 'Создать видео';

  @override
  String get terminalUnavailable => 'Терминал не подключен';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Обновить';

  @override
  String get loading => 'Загрузка';

  @override
  String get home => 'Чаты';

  @override
  String get idle => 'Ожидание';

  @override
  String get allProjects => 'Все проекты';

  @override
  String get allWorktrees => 'Все рабочие деревья';

  @override
  String get filterProjects => 'Фильтр проектов';

  @override
  String get closeSearch => 'Закрыть поиск';

  @override
  String onlineHostCount(String count) {
    return '$count онлайн';
  }

  @override
  String get taskActions => 'Действия по задачам';

  @override
  String get archiveShort => 'Архив';

  @override
  String get archiveTab => 'Архивировано';

  @override
  String get archivedTasks => 'Архивировано';

  @override
  String get delete => 'Удалить';

  @override
  String get deleteTask => 'Удалить чат';

  @override
  String get deleteWarning =>
      'Этот чат не может быть возобновлен после удаления';

  @override
  String get busyDelete => 'Остановить задачу перед удалением этого чата';

  @override
  String get stopBeforeDelete => 'Остановить задачу';

  @override
  String get deleted => 'Чат удален из предварительного просмотра';

  @override
  String get restored => 'Возвращение домой';

  @override
  String get restore => 'Восстановить';

  @override
  String get archiveEmpty => 'Нет архивированных чатов';

  @override
  String get archiveKeepsRunning =>
      'Архивация не останавливает выполнение заданий';

  @override
  String get title => 'Sailry · Мобильный просмотр';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Мобильная рабочая зона';

  @override
  String get edition => 'Мобильное исследование / 01';

  @override
  String get intro => 'Задачи, чаты и удаленные рабочие места';

  @override
  String get preview => 'Предварительный просмотр';

  @override
  String get sample => 'Образцы данных · Изменения остаются на этой странице';

  @override
  String get mixed => 'Свет и темнота';

  @override
  String get dark => 'Тёмная';

  @override
  String get light => 'Светлая';

  @override
  String get gallery => 'Общий обзор';

  @override
  String get focus => 'Один экран';

  @override
  String get reset => 'Сбросить предварительный просмотр';

  @override
  String get page => 'Выберите страницу';

  @override
  String get experience => 'Открыть страницу';

  @override
  String get backGallery => 'Вернуться к обзору';

  @override
  String get design => 'Особенности и дизайн';

  @override
  String get footer => 'ПАРАШЮТНЫЙ/МОБИЛЬНЫЙ';

  @override
  String get footerNote =>
      'Локальный предварительный просмотр HTML · Отсутствие подключения к службе';

  @override
  String get tasks => 'Задачи';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Хосты';

  @override
  String get resources => 'Ресурсы';

  @override
  String get settings => 'Настройки';

  @override
  String get usage => 'Использование';

  @override
  String get changes => 'Изменения';

  @override
  String get terminal => 'Терминал';

  @override
  String get newTerminal => 'Новый терминал';

  @override
  String get files => 'Файлы';

  @override
  String get project => 'Проект';

  @override
  String get worktree => 'Рабочее дерево';

  @override
  String get subtitleTasks =>
      'Задачи между узлами · Сначала утверждения и ответы';

  @override
  String get subtitleChat =>
      'Постоянный чат · Расширение активности инструмента по мере необходимости';

  @override
  String get subtitleHosts => 'Подключения, ресурсы узла и процессы';

  @override
  String get subtitleResources => 'Узел → Проект → Рабочее дерево';

  @override
  String get subtitleChanges => 'Различия файлов, подготовка и фиксации';

  @override
  String get subtitleTerminal => 'Удаленный терминал · Явное управление входом';

  @override
  String get subtitleUsage => 'Sailry чатов · Агрегированные по хостам';

  @override
  String get subtitleSettings =>
      'Локальные настройки и параметры выполнения Node';

  @override
  String get allHosts => 'Все хосты';

  @override
  String get connectedHosts => '2 онлайн';

  @override
  String get all => 'Все';

  @override
  String get running => 'Выполняется';

  @override
  String get waiting => 'В ожидании';

  @override
  String get completed => 'Завершено';

  @override
  String get taskProgress => 'Текущая задача';

  @override
  String get taskWait => 'В ожидании вашего решения';

  @override
  String get taskRecent => 'Недавно завершенные';

  @override
  String get search => 'Поиск';

  @override
  String get searchTasks => 'Поиск задач и проектов';

  @override
  String get filterTasks => 'Задачи фильтра';

  @override
  String get noResults => 'Нет соответствующих задач';

  @override
  String get notification => 'Уведомления';

  @override
  String get newTask => 'Новая задача';

  @override
  String get newConversation => 'Новый чат';

  @override
  String get approveTitle => 'Обновить макет входа';

  @override
  String get approveNote => 'Запуск тестов проекта';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'Организация API документации';

  @override
  String get questionNote => 'Ожидается ответ';

  @override
  String get question => 'На каком языке должна составляться документация?';

  @override
  String get optionChinese => 'Китайский язык';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Ответ';

  @override
  String get approval => 'Официальное утверждение';

  @override
  String get viewRequest => 'Посмотреть запрос';

  @override
  String get taskSearch => 'Улучшение поиска файлов';

  @override
  String get taskSearchNote => 'Проверка индекса каталога';

  @override
  String get taskTest => 'Fix восстановление чата';

  @override
  String get taskTestNote => 'Испытания на ход';

  @override
  String get taskDone => 'Обновить README проекта';

  @override
  String get taskDoneNote => '3 файла изменены';

  @override
  String get ago => 'Только сейчас';

  @override
  String get minutesAgo => '12 минут назад';

  @override
  String get allow => 'Разрешить один раз';

  @override
  String get deny => 'Запретить';

  @override
  String get approved => 'Разрешено · Образец';

  @override
  String get denied => 'Отказано · Образец';

  @override
  String get answered => 'Ответил · Образец';

  @override
  String get awaiting => 'Ожидается утверждение';

  @override
  String get working => 'В работе';

  @override
  String get viewChanges => 'Просмотр изменений';

  @override
  String get viewConversation => 'Посмотреть чат';

  @override
  String get chatTitle => 'Обновить макет входа';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Сегодня 09:36';

  @override
  String get userMessage =>
      'Настройка интервалов между входами и унификация стилей ввода и кнопок при сохранении логики входа';

  @override
  String get assistantMessage =>
      'Я проверил страницу входа и общие компоненты формы, унифицированные интервалы ввода и добавленные стили фокуса клавиатуры';

  @override
  String get replyPreview => 'Образец потока задач';

  @override
  String get phaseThinking => 'Мысли';

  @override
  String get phaseReading => 'Чтение файлов';

  @override
  String get phaseQuestion => 'Ожидается ответ';

  @override
  String get phaseEditing => 'Редактирование файлов';

  @override
  String get phaseApproval => 'Ожидается утверждение';

  @override
  String get phaseTesting => 'Испытания на ход';

  @override
  String get phaseReply => 'Реагирование';

  @override
  String get phaseFollowup => 'Очередь обработки';

  @override
  String get phaseComplete => 'Завершено';

  @override
  String get phaseFailed => 'Неудачные испытания';

  @override
  String get allowShort => 'Разрешить';

  @override
  String get queueShort => 'Очередь';

  @override
  String get confirmShort => 'Подтвердить';

  @override
  String get todoShort => 'Что делать';

  @override
  String get todoInspect => 'Проверьте страницу входа';

  @override
  String get todoEdit => 'Настройка стилей формы';

  @override
  String get todoTest => 'Запуск тестов проекта';

  @override
  String get todoNarrow => 'Проверка узкого экранного расстояния';

  @override
  String get workProcess => 'Активность';

  @override
  String workSteps(String count) {
    return '· $count шагов';
  }

  @override
  String get questionRecord => 'Подтвердить макет';

  @override
  String get answerRecorded => 'ответил';

  @override
  String get playFlow => 'Задача игры';

  @override
  String get pauseFlow => 'Пауза демо';

  @override
  String get nextFlow => 'Следующий шаг';

  @override
  String get replyingNow => 'Реагирование';

  @override
  String get toolReadLabel => 'Чтение';

  @override
  String get toolEditLabel => 'Изменить';

  @override
  String get toolRunLabel => 'Выполнить';

  @override
  String get readGroup => '3 ряда';

  @override
  String get readFileResult => 'Файл считан';

  @override
  String get readFileProgress => 'Чтение файла';

  @override
  String get flowAttachment =>
      'Обновление входа: унифицировать промежутки между формами, добавить стили фокуса клавиатуры и сохранить логику входа';

  @override
  String get readResult =>
      'Чтение файла Login.tsx и общих стилей форм\nШирина мобильной кнопки отличается от формы';

  @override
  String get layoutFindings =>
      'Форма входа использует пробел рабочего стола, а мобильная кнопка не заполняет свой контейнер';

  @override
  String get layoutQuestion =>
      'Должна ли кнопка мобильного входа заполнять всю ширину?';

  @override
  String get questionPending => 'В ожидании вашего ответа';

  @override
  String get wideButton => 'Использовать кнопку полной ширины';

  @override
  String get keepButton => 'Сохранить текущую ширину';

  @override
  String get editPlan =>
      'Я буду сохранять логику входа, унифицировать пробел и сделать мобильную кнопку полной ширины';

  @override
  String get editPlanKeep =>
      'Я сохраню ширину кнопки и логику входа, настраивая только пробел и фокус стили';

  @override
  String get editThinking =>
      'Повторное использование существующих переменных стиля и ограничение изменений макета на форму входа';

  @override
  String get editResult =>
      'Обновлено 3 файла\nДобавлены стили фокуса и правила мобильного макета';

  @override
  String get beforeTest =>
      'Изменения макета завершены. Далее я запущу тесты проекта, чтобы проверить регрессии';

  @override
  String get testTool => 'Запуск тестов проекта';

  @override
  String get testProgress =>
      'Запуск тестов формы входа…\nПроверка фокуса и взаимодействия клавиатуры';

  @override
  String get testResult =>
      '12 сдавших тесты\nНе найдено логических регрессий входа';

  @override
  String get testFailure =>
      'Проверка порядка фокусировки не удалась\nОжидаемый фокус на поле пароля, но он остался на поле имени пользователя';

  @override
  String get testFailed => 'Неудачные испытания';

  @override
  String get flowResult =>
      'Все 12 тестов прошли успешно, и логика входа в систему осталась неизменной';

  @override
  String get queueSample =>
      'Проверьте расстояние между кнопками и на узких экранах';

  @override
  String queueCount(String count) {
    return '$count сообщений в очереди';
  }

  @override
  String queuePaused(String count) {
    return 'Очередь приостановлена · $count';
  }

  @override
  String get pauseQueue => 'Очередной перерыв';

  @override
  String get resumeQueue => 'Возобновление очереди';

  @override
  String get sendNext => 'Отправить следующий';

  @override
  String get enqueue => 'Добавить в очередь';

  @override
  String get queuedPreview => 'Добавлено в очередь образцов';

  @override
  String get moveUp => 'Движение вверх';

  @override
  String get followupThinking =>
      'Проверка существующих точек останова для подтверждения последовательного расположения кнопок на узких экранах';

  @override
  String get followupTool => 'Проверка узких стилей экрана';

  @override
  String get followupToolResult =>
      '320px и 390px используют одни и те же правила промежутков';

  @override
  String get followupResult =>
      'Последовательное расположение кнопок на узком экране; никаких дальнейших изменений не требуется';

  @override
  String get deniedResult =>
      'Тестирование не проводилось; текущие изменения сохранены';

  @override
  String get thinkingNow => 'Мысли';

  @override
  String get toolsNow => 'Выполнение';

  @override
  String get toolPending => 'Не начато';

  @override
  String get thoughtLive =>
      'Сначала проверьте страницу входа и компоненты формы, чтобы выявить изменения в промежутках и фокусе';

  @override
  String get toolsShort => '3 действия';

  @override
  String get thought => 'Обоснование';

  @override
  String get thoughtContent =>
      'Повторное использование существующих компонентов формы и настройка только макета входа и стилей фокуса';

  @override
  String get toolsComplete => '3 мероприятия завершены';

  @override
  String get toolRead => 'Чтение компонентов входа и формы';

  @override
  String get toolEdit => 'Обновление стилей пробелов и фокуса';

  @override
  String get toolDiff => 'Проверка различий файлов';

  @override
  String get changedFiles => '3 файла изменены';

  @override
  String get approvalBody =>
      'Запуск тестов в рабочем дереве sailry-web в Studio';

  @override
  String get approvalResolved => 'Утверждение решено';

  @override
  String get chatContinue => 'Продолжайте описывать свою задачу';

  @override
  String get describeTask => 'Опишите свою задачу';

  @override
  String get send => 'Отправить';

  @override
  String get attach => 'Прикрепить';

  @override
  String get voice => 'Голосовой ввод';

  @override
  String get voiceNote =>
      'Этот предварительный просмотр не использует микрофон';

  @override
  String get attachmentNote => 'Образец приложения добавлен';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Удалить вложение';

  @override
  String get sentPreview => 'Только предварительный просмотр, не отправляется';

  @override
  String get model => 'Модель';

  @override
  String get modelSource => 'Текущая конфигурация чата · Студия';

  @override
  String get copy => 'Копировать';

  @override
  String get copied => 'Копировано';

  @override
  String get copyFailed => 'Копия отсутствует; выберите текст вручную';

  @override
  String get more => 'Ещё';

  @override
  String get close => 'Закрыть';

  @override
  String get back => 'Назад';

  @override
  String get cancel => 'Отмена';

  @override
  String get save => 'Сохранить';

  @override
  String get select => 'Выбрать';

  @override
  String get sessionActions => 'Действия чата';

  @override
  String get queue => 'Очередь сообщений';

  @override
  String get queueEmpty => 'Нет сообщений в очереди';

  @override
  String get fork => 'Создать ветку чата';

  @override
  String get forked => 'Создан образец вилки';

  @override
  String get archive => 'Архив чата';

  @override
  String get archived => 'Архивировано в предварительном просмотре';

  @override
  String get stop => 'Остановить задачу';

  @override
  String get stopped => 'Задание остановлено в предварительном просмотре';

  @override
  String get stoppedStatus => 'Остановлено';

  @override
  String get hostSubtitle => 'Ваши узлы исполнения';

  @override
  String get pair => 'Подключить узел';

  @override
  String get online => 'В сети';

  @override
  String get offline => 'Не в сети';

  @override
  String get connection => 'Соединение';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 цветов';

  @override
  String get laptopSystem => 'Последний раз онлайн 2 часа назад';

  @override
  String get statusHealthy => 'Здоровые';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Память';

  @override
  String get disk => 'Диск';

  @override
  String get metrics => 'Использование ресурсов';

  @override
  String get activity => 'Активность';

  @override
  String get lastHour => 'Последние 60 минут';

  @override
  String get sessionCount => 'Чаты';

  @override
  String get terminalCount => 'Терминалы';

  @override
  String get projectCount => 'Проекты';

  @override
  String get processes => 'Процессы';

  @override
  String get process => 'Имя';

  @override
  String get network => 'Сеть';

  @override
  String get details => 'Подробности';

  @override
  String get manageHost => 'Хост Детали';

  @override
  String get hostProjects => 'Принимающие проекты';

  @override
  String get connectionDetails => 'Подробности подключения';

  @override
  String get direct => 'Прямые';

  @override
  String get relay => 'Релейная связь';

  @override
  String get latency => 'Задержка';

  @override
  String get hostOffline =>
      'Узел отключен; показывает свое последнее известное состояние';

  @override
  String get retry => 'Повторить';

  @override
  String get retryNote =>
      'Предварительный просмотр не подключен к реальному узлу';

  @override
  String get pairTitle => 'Подключение узла';

  @override
  String get pairDescription =>
      'Введите 6-значный код сопряжения, указанный на узле';

  @override
  String get pairCode => 'Парный код';

  @override
  String get pairHint => 'Парный код истекает через 60 секунд';

  @override
  String get pairDemo => 'Имитировать подключение';

  @override
  String get pairSuccess => 'Пример хоста добавлен';

  @override
  String get pairInvalid => 'Введите 6 цифр';

  @override
  String get workspace => 'Рабочее пространство';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Выберите хост';

  @override
  String get selectProject => 'Выберите проект';

  @override
  String get selectBranch => 'Выберите рабочее дерево';

  @override
  String get mainBranch => 'Основное рабочее дерево';

  @override
  String get featureBranch => 'Компоновка входа';

  @override
  String get connectionTools => 'Соединения';

  @override
  String get workspaceResources => 'Рабочее пространство';

  @override
  String get confirm => 'Подтвердить';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Изменения, ветви и история';

  @override
  String get gitBranches => 'Ветки';

  @override
  String get gitHistory => 'История';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added строк добавлено · $removed удалено';
  }

  @override
  String get gitActions => 'Git акции';

  @override
  String get gitFetch => 'Извлечение';

  @override
  String get gitPull => 'Пулл';

  @override
  String get gitPush => 'Пуш';

  @override
  String get gitCurrent => 'Нынешний филиал';

  @override
  String get gitCreateBranch => 'Новое отделение';

  @override
  String get gitBranchName => 'Название филиала';

  @override
  String get gitSwitch => 'Коммутаторная ветка';

  @override
  String get gitMerge => 'Слияние ответвлений';

  @override
  String get gitDeleteBranch => 'Исключить ветвь';

  @override
  String get gitHistoryLayout => 'Настройка промежутков между формами входа';

  @override
  String get gitHistoryInit => 'Инициализация страницы входа';

  @override
  String get gitPreview => 'Git только моделирование; хранилище не изменено';

  @override
  String get gitDirty => 'Сначала фиксировать текущие изменения';

  @override
  String get gitSwitchNote =>
      'Переключить ветку в этом рабочем дереве; только моделирование';

  @override
  String get gitDeleteNote => 'Удалить выбранную ветку; только моделирование';

  @override
  String get gitInvalidBranch => 'Неверное имя или ветка уже существует';

  @override
  String get review => 'Проверка';

  @override
  String get browseFiles => 'Просмотр рабочего дерева';

  @override
  String get reviewFiles => 'Просмотр изменений кода';

  @override
  String get selectWorkspace => 'Проект и рабочее дерево';

  @override
  String get resourceSummary => '2 чата · 1 терминал';

  @override
  String get searchFiles => 'Поиск файлов';

  @override
  String get recentFiles => 'Файлы';

  @override
  String get src => 'Источник';

  @override
  String get folder => 'Папка';

  @override
  String get modified => 'Изменено';

  @override
  String get filePreview => 'Просмотр файла';

  @override
  String get fileSample => 'Образец содержимого файла';

  @override
  String get edit => 'Изменить';

  @override
  String get savePreview =>
      'Изменения, сохраненные в этом предварительном просмотре';

  @override
  String get unsaved => 'Не сохранено';

  @override
  String get discard => 'Отбросить изменения';

  @override
  String get discardConfirm =>
      'Отбросить несохраненные изменения в этом файле?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'Больше ресурсов';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'База данных';

  @override
  String get ports => 'Перенаправление портов';

  @override
  String get browser => 'Веб-предварительный просмотр';

  @override
  String get portsSub => '1 передний';

  @override
  String get connectionOwner => 'Исполнение Node · Студия';

  @override
  String get openTerminal => 'Открытый терминал';

  @override
  String get tables => 'Таблицы';

  @override
  String get portNote =>
      'Переадресация образца · Нет локального прослушивателя порта';

  @override
  String get portTarget => 'Целевой порт';

  @override
  String get localPort => 'Местный порт';

  @override
  String get closePort => 'Закрыть вперед';

  @override
  String get portClosed => 'Образец вперед закрыт';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Рабочее дерево';

  @override
  String get staged => 'Сценарист';

  @override
  String get diffSummary => '3 ряда';

  @override
  String get stage => 'Этап все';

  @override
  String get unstage => 'Отступление';

  @override
  String get commit => 'Коммит';

  @override
  String get commitTitle => 'Закрепить изменения';

  @override
  String get commitMessage => 'Сообщение фиксации';

  @override
  String get commitPlaceholder => 'Опишите изменения';

  @override
  String get commitPreview => 'Симулировать коммит';

  @override
  String get committed => 'Завершение фиксации образца';

  @override
  String get noChanges => 'Нет изменений для фиксации';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Продолжить редактирование';

  @override
  String get diffSelection => 'Выберите измененный файл';

  @override
  String get terminalKeyboard => 'Клавиатура';

  @override
  String get terminalEnter => 'Enter';

  @override
  String get terminalOutputLabel => 'Выход терминала';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Только чтение';

  @override
  String get takeControl => 'Взять под контроль';

  @override
  String get hasControl => 'Контроль ввода';

  @override
  String get releaseControl => 'Контроль выбросов';

  @override
  String get terminalPlaceholder => 'Введите примерную команду';

  @override
  String get terminalPreview => 'Пример терминала · Команды не выполняются';

  @override
  String get terminalOutput =>
      'Команда получена в предварительном виде, не выполнена';

  @override
  String get terminalControlNote =>
      'Принятие контроля для отправки ввода; имитируется здесь';

  @override
  String get usageSubtitle => 'Sailry только чаты';

  @override
  String get week => 'На этой неделе';

  @override
  String get month => 'В этом месяце';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total ответов';
  }

  @override
  String get usageEmpty => 'Данные об использовании отсутствуют';

  @override
  String get estimatedCost => 'Сметные расходы';

  @override
  String get costCoverage => '42 / 48 ответов';

  @override
  String get partial => 'Частичные данные';

  @override
  String get sourcesPartial => '2 / 3 хоста обновлены';

  @override
  String get input => 'Ввод';

  @override
  String get output => 'Вывод';

  @override
  String get cached => 'Поиски в кэше';

  @override
  String get modelUsage => 'Распределение по модели';

  @override
  String get hostUsage => 'Использование хоста';

  @override
  String get recentRequests => 'Последние ответы';

  @override
  String get allUsage => 'Детали использования';

  @override
  String get usageNote =>
      'Расходы являются оценочными; некоторые ответы не имеют цены';

  @override
  String get sourceNote =>
      'Автономные узлы сохраняют свои последние известные данные';

  @override
  String get profileSubtitle => 'Мобильный контроллер';

  @override
  String get localSettings => 'Местные предпочтения';

  @override
  String get nodeSettings => 'Настройки выполнения Node';

  @override
  String get appearance => 'Внешний вид';

  @override
  String get notifications => 'Уведомления';

  @override
  String get enabled => 'Вкл';

  @override
  String get disabled => 'Выкл';

  @override
  String get add => 'Добавить';

  @override
  String get configName => 'Имя';

  @override
  String get configEndpoint => 'Конечная точка';

  @override
  String get configModels => 'Модели';

  @override
  String get configInstructions => 'Инструкции';

  @override
  String get configContent => 'Содержание';

  @override
  String get configEmpty => 'Нет записей';

  @override
  String get configDuplicate => 'Имя уже существует';

  @override
  String configDelete(String name) {
    return 'Исключить \"$name\"?';
  }

  @override
  String get speechInput => 'Голосовой ввод';

  @override
  String get developerInstructions =>
      'Изменить код для задачи и проверить результат';

  @override
  String get reviewerInstructions =>
      'Обзор изменений в коде и выявление проблем';

  @override
  String get projectConventions => 'Проектные конвенции';

  @override
  String get memoryContent => 'Сохранить существующий стиль кода';

  @override
  String get providers => 'Модели и поставщики';

  @override
  String get roles => 'Роли';

  @override
  String get memorySettings => 'Память';

  @override
  String get speech => 'Речь';

  @override
  String nodeSettingsNote(String host) {
    return 'Конфигурация сохранена на $host';
  }

  @override
  String get about => 'О Sailry';

  @override
  String get aboutBody =>
      'Предварительный просмотр мобильного взаимодействия, не подключен к службам';

  @override
  String get settingsSaved =>
      'Настройки, обновленные в этом предварительном просмотре';

  @override
  String get modelPicker => 'Выберите модель';

  @override
  String get nodeDefaults => 'Node по умолчанию';

  @override
  String get providerNote =>
      'Пример конфигурации · Учетные данные остаются при выполнении Node';

  @override
  String get roleNote => 'Пример роли · Применяется к новым чатам';

  @override
  String get auto => 'Автоматически';

  @override
  String get manual => 'Задавайте каждый раз';

  @override
  String get notificationsNote =>
      'Только уведомления предварительного просмотра';

  @override
  String get memoryNote => 'Образец Node памяти';

  @override
  String get speechNote => 'Использует конфигурацию речи выполнения Node';

  @override
  String get newTaskHost => 'Узел выполнения';

  @override
  String get newTaskProject => 'Проект';

  @override
  String get newTaskWorktree => 'Рабочее дерево';

  @override
  String get create => 'Создать';

  @override
  String get taskCreated => 'Образец чата создан';

  @override
  String get required => 'Опишете сначала свою задачу';

  @override
  String get notificationsEmpty => 'Новых уведомлений нет';

  @override
  String get reviewTitle => 'Справочные материалы по конструкции';

  @override
  String get reviewIntro =>
      'Страницы следуют текущему источнику; этот предварительный просмотр не устанавливает принятие мобильного сервиса';

  @override
  String get reviewConversation => 'Чаты, утверждения, вопросы и очередь';

  @override
  String get reviewConversationText =>
      'Задачи сохраняют владение хостом, проектом и рабочим деревом; расширяют записи инструментов и утверждения в чатах';

  @override
  String get reviewResources => 'Файл, Git, клемм и соединений';

  @override
  String get reviewResourcesText =>
      'Редактирование файлов, подготовка, фиксации и порты сохраняют свои входные точки; подробности открываются на вторичных страницах';

  @override
  String get reviewHosts => 'Подключения к хостам и мониторинг';

  @override
  String get reviewHostsText =>
      'Парные узлы, 6-значные коды, использование ресурсов и процессы; автономное состояние не отображается как активное';

  @override
  String get reviewUsage => 'Использование и параметры Node';

  @override
  String get reviewUsageText =>
      'Sailry только чаты; агрегированные данные об использовании сохраняют полноту, а расходы указывают на оценки и охват';

  @override
  String get reviewBoundary => 'Подвижная граница';

  @override
  String get reviewBoundaryText =>
      'Мобильный мост подвергает подключения, чаты, терминалы и использование; этот предварительный просмотр начинается с № Node, моделей, парирования, терминалов или плагинов';

  @override
  String get reviewVisual => 'Визуальные ссылки';

  @override
  String get reviewVisualText =>
      'Справочная информация 1: иерархия чата; справочная информация 2: мягкие карты и плавающая навигация; справочная информация 3: компактный мониторинг';

  @override
  String get hostConnectPrompt => 'Подключение узла';

  @override
  String get hostDisconnected => 'Соединение прервано';

  @override
  String get language => 'Язык';

  @override
  String get languageSystem => 'Система';

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
  String get backgroundConnection => 'Подключение в фоновом режиме';

  @override
  String get backgroundConnectionActive =>
      'Обеспечение активных подключений узлов';

  @override
  String get backgroundConnectionFailed =>
      'Фоновое подключение не включено; повторите попытку';

  @override
  String get resetReasoning => 'Усилия по сбросу';

  @override
  String get completionAlerts => 'Уведомления о завершении';

  @override
  String get notificationsReadAll => 'Отметить все как прочитанные';

  @override
  String get notificationsOpen => 'Открыть';

  @override
  String get preferencesFailed => 'Настройки не сохранены; повторите попытку';

  @override
  String get connectFirst => 'Подключите узел, чтобы начать';

  @override
  String get initializing => 'Запуск';

  @override
  String get startupFailed => 'Не удалось запустить';

  @override
  String get retryConnection => 'Повторить';

  @override
  String get pairAction => 'Подключить';

  @override
  String get pairFailed => 'Подключение не удалось; повторите попытку';

  @override
  String get pairExpired =>
      'Срок действия парольного кода истек; получите новый код';

  @override
  String get pairing => 'Подключение';

  @override
  String get hostUnavailable => 'Узел не подключен';

  @override
  String get hostMetricsFailed => 'Не удается прочитать состояние узла';

  @override
  String get hostProcessesEmpty => 'Отсутствие процессов';

  @override
  String get hostRegisterProject => 'Добавить проект';

  @override
  String get hostChooseDirectory => 'Выберите каталог';

  @override
  String get hostChooseFile => 'Выберите файл';

  @override
  String get hostParentDirectory => 'Родительский каталог';

  @override
  String get hostEmptyDirectory => 'Каталог пуст';

  @override
  String get hostLoadMore => 'Загрузить больше';

  @override
  String get hostProjectName => 'Название проекта';

  @override
  String get hostProjectPath => 'Путь проекта на узле';

  @override
  String get hostProjectFailed => 'Не удается добавить проект';

  @override
  String get hostUnknown => 'Данные отсутствуют';

  @override
  String get hostRefresh => 'Обновить';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Память';

  @override
  String get hostMetricDisk => 'Диск';

  @override
  String get failureConflict =>
      'Содержимое изменено; перезагрузка и повторная попытка';

  @override
  String get failureUnknown =>
      'Результат не подтвержден; сначала проверьте состояние узла';

  @override
  String get failureDenied => 'Разрешение отказано';

  @override
  String get failureUnavailable => 'Узел не подключен';

  @override
  String get failureBusy => 'Служба занята; повторите попытку позже';

  @override
  String get failureGeneric => 'Операция не удалась';

  @override
  String get settingsSpeechLanguage => 'Язык';

  @override
  String get settingsSpeechAuto => 'Определять автоматически';

  @override
  String get settingsSpeechChinese => 'Китайский язык';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Готова модель речи';

  @override
  String get settingsSpeechDownload => 'Загрузить модель речи';

  @override
  String get settingsSpeechFailed => 'Модель речи не готова; повторите попытку';

  @override
  String get settingsNoHost => 'Сначала подключите узел';

  @override
  String get settingsUnavailable => 'Недоступно';

  @override
  String get settingsLoadFailed => 'Не удалось загрузить';

  @override
  String get settingsSaveFailed => 'Сохранить не удалось; проект сохранен';

  @override
  String get settingsConflict =>
      'Настройки изменены; заново откройте и повторите попытку';

  @override
  String get settingsUnknown =>
      'Результат не подтвержден; обновить для проверки';

  @override
  String get settingsRetry => 'Повторить';

  @override
  String get settingsLoading => 'Загрузка';

  @override
  String get settingsRequired => 'Введите значение';

  @override
  String get settingsKey => 'Идентификация';

  @override
  String get settingsDescription => 'Описание';

  @override
  String get settingsInstructions => 'Инструкции';

  @override
  String get settingsModels =>
      'Идентификационные номера моделей, по одному на строку';

  @override
  String get settingsApi => 'API формат';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'Ключ API';

  @override
  String get settingsEnabled => 'Включено';

  @override
  String get settingsDriver => 'Служба';

  @override
  String get settingsModel => 'Модель';

  @override
  String get settingsMemoryAuto => 'Автоматическая запись';

  @override
  String get settingsMemoryBudget => 'Контекстные байт';

  @override
  String get settingsMemoryReview => 'Периодичность обзоров (в днях)';

  @override
  String get settingsMemoryRecords => 'Записи в памяти';

  @override
  String get settingsMemoryKind => 'Тип';

  @override
  String get settingsMemoryUser => 'Пользователь';

  @override
  String get settingsMemoryFeedback => 'Обратная связь';

  @override
  String get settingsMemoryProject => 'Проект';

  @override
  String get settingsMemoryReference => 'Справочная информация';

  @override
  String get settingsArchived => 'Архивировано';

  @override
  String get settingsEmpty => 'Нет записей';

  @override
  String get settingsUsageUnknown => 'Неизвестно';

  @override
  String get settingsUsagePartial => 'Некоторые узлы недоступны';

  @override
  String get settingsUsageCache => 'Кэшированный';

  @override
  String get settingsUsageInput => 'Некэшированный ввод';

  @override
  String get settingsUsageOutput => 'Вывод';

  @override
  String get settingsUsageDaily => 'Ежедневно';

  @override
  String get settingsUsageWeekly => 'Еженедельно';

  @override
  String get settingsUtc => 'UTC';
}
