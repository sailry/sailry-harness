// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for German (`de`).
class AppLocalizationsDe extends AppLocalizations {
  AppLocalizationsDe([String locale = 'de']) : super(locale);

  @override
  String get updatesVersion => 'Version';

  @override
  String get updatesCheck => 'Nach Updates suchen';

  @override
  String get updatesChecking => 'Wird geprüft';

  @override
  String updatesAvailable(String version) {
    return 'Version $version ist verfügbar';
  }

  @override
  String get updatesDownload => 'Update herunterladen';

  @override
  String get updatesCurrent => 'Sie sind auf dem Laufenden';

  @override
  String get updatesUnpublished => 'Noch keine mobile Version';

  @override
  String get updatesCheckFailed => 'Konnte nicht nach Updates suchen';

  @override
  String get updatesOpenFailed => 'Der Download konnte nicht geöffnet werden';

  @override
  String get retryTask => 'Erneut versuchen';

  @override
  String get welcomeTitle => 'Woran möchten Sie heute arbeiten?';

  @override
  String get welcomeExplore => 'Ein Projekt durchsuchen';

  @override
  String get welcomeExploreDetail => 'Struktur und Einstiegspunkte verstehen';

  @override
  String get welcomeExplorePrompt =>
      'Helfen Sie mir, dieses Projekt zu verstehen, einschließlich seiner Schlüsselmodule und Einstiegspunkte.';

  @override
  String get welcomeBuild => 'Baue eine Idee';

  @override
  String get welcomeBuildDetail => 'Erwecken Sie Ihre Idee zum Leben';

  @override
  String get welcomeBuildPrompt =>
      'Ich möchte ein Feature zu diesem Projekt hinzufügen, bitte bestätigen Sie zuerst die Anforderungen mit mir und skizzieren Sie einen Implementierungsplan.';

  @override
  String get welcomeReview => 'Änderungen überprüfen';

  @override
  String get welcomeReviewDetail =>
      'Änderungen und potenzielle Probleme prüfen';

  @override
  String get welcomeReviewPrompt =>
      'Überprüfen Sie die aktuellen Änderungen in diesem Projekt und konzentrieren Sie sich auf mögliche Probleme und fehlende Tests.';

  @override
  String get welcomePlan => 'Machen Sie einen Plan';

  @override
  String get welcomePlanDetail => 'Ziele und Schritte klären';

  @override
  String get welcomePlanPrompt =>
      'Helfen Sie mir, einen Schritt-für-Schritt-Plan für die anstehenden Entwicklungsarbeiten zu erstellen.';

  @override
  String get conversationEmpty => 'Beschreiben Sie Ihre Aufgabe';

  @override
  String get conversationLoading => 'Chat wird geladen';

  @override
  String get conversationReconnecting => 'Verbindung wird wiederhergestellt';

  @override
  String get conversationErrorDetails => 'Grund anzeigen';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Wiederholung der Modellanfrage $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Modellanfrage wiederholt $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count Werkzeugaufrufe';
  }

  @override
  String get conversationGoal => 'Ziel';

  @override
  String get conversationGoalBlocked => 'Blockiert';

  @override
  String conversationGoalBudget(String count) {
    return 'Budget $count Token';
  }

  @override
  String get conversationStepSkipped => 'Übersprungen';

  @override
  String get conversationChild => 'Unteraufgabe';

  @override
  String get conversationChildReadonly => 'Subtask-Chat';

  @override
  String get conversationOffline => 'Verbindung verloren';

  @override
  String get conversationUnavailable => 'Chat nicht verfügbar';

  @override
  String get conversationFailed =>
      'Vorgang fehlgeschlagen; versuchen Sie es erneut';

  @override
  String get conversationUnknown =>
      'Ergebnis unbestätigt; überprüfen Sie den Chat, bevor Sie erneut handeln';

  @override
  String get conversationCheckResult => 'Ergebnis prüfen';

  @override
  String get conversationConflict =>
      'Konfiguration geändert; erneut öffnen, bevor Sie erneut handeln';

  @override
  String get conversationOlder => 'Ältere Nachrichten laden';

  @override
  String get conversationNew => 'Neuen Chat erstellen';

  @override
  String get conversationNoHost => 'Verbinden Sie zuerst einen Host';

  @override
  String get conversationNoProject =>
      'Zuerst ein Projekt auf dem Host hinzufügen';

  @override
  String get conversationNoModel =>
      'Konfigurieren Sie zuerst ein Modell auf dem Host';

  @override
  String get conversationNoTasks => 'Noch keine Chats';

  @override
  String get conversationNoMessages => 'Keine Nachrichten gefunden';

  @override
  String get conversationPreviewUnavailable => 'Nachricht nicht verfügbar';

  @override
  String get conversationInterrupted => 'Unterbrochen';

  @override
  String get conversationFailedStatus => 'Fehlgeschlagen';

  @override
  String get conversationStopping => 'Wird gestoppt';

  @override
  String get conversationQueued => 'Warteschlange';

  @override
  String get conversationProcessing => 'Verarbeitung';

  @override
  String get conversationUnsynced => 'Status nicht synchronisiert';

  @override
  String get conversationGenerating => 'Antworten';

  @override
  String get conversationWaiting => 'Warten auf Bestätigung';

  @override
  String get conversationCompacting => 'Kompaktierung des Kontexts';

  @override
  String get conversationForkConfirm =>
      'Einen Chat aus diesem Datensatz forken?';

  @override
  String get conversationCompacted => 'Kontext komprimiert';

  @override
  String get conversationToolWaiting => 'Ausstehend';

  @override
  String get conversationToolRunning => 'Wird ausgeführt';

  @override
  String get conversationToolReturned => 'Zurückgegeben';

  @override
  String get conversationToolCancelled => 'Abgebrochen';

  @override
  String get conversationToolNotExecuted => 'Nicht ausgeführt';

  @override
  String get conversationToolInterrupted => 'Unterbrochen';

  @override
  String get conversationUnsupportedInput =>
      'Diese Eingabe auf dem Desktop behandeln';

  @override
  String get conversationStartCoding => 'Ausführung starten';

  @override
  String get conversationPlanFeedback => 'Änderungen vorschlagen';

  @override
  String get conversationOther => 'Andere';

  @override
  String get conversationSubmit => 'Absenden';

  @override
  String get conversationSource => 'Quelle';

  @override
  String get conversationMode => 'Arbeitsmodus';

  @override
  String get conversationCode => 'Ausführen';

  @override
  String get conversationPlan => 'Plan';

  @override
  String get conversationPermission => 'Berechtigungen';

  @override
  String get conversationAsk => 'Fragen Sie jedes Mal';

  @override
  String get conversationProject => 'Zugriff auf Projekte';

  @override
  String get conversationFull => 'Vollständiger Zugriff auf';

  @override
  String get conversationReasoning => 'Argumentationsaufwand';

  @override
  String get conversationDefault => 'Standard';

  @override
  String get conversationNone => 'Aus';

  @override
  String get conversationMinimal => 'Minimal';

  @override
  String get conversationLow => 'Niedrig';

  @override
  String get conversationMedium => 'Mittel';

  @override
  String get conversationHigh => 'Hoch';

  @override
  String get conversationXHigh => 'Höher';

  @override
  String get conversationMax => 'Maximum';

  @override
  String get conversationBudget => 'Begründung des Haushaltsplans';

  @override
  String get conversationAttachment => 'Anhang';

  @override
  String get conversationAttachmentTooLarge =>
      'Anhang unleserlich oder größer als 64 MB';

  @override
  String get conversationDownload => 'Anhang anzeigen';

  @override
  String get conversationImageFailed => 'Bild kann nicht angezeigt werden';

  @override
  String get conversationDownloadFailed => 'Anhang konnte nicht geladen werden';

  @override
  String get conversationReadonly => 'Dieser Chat ist archiviert';

  @override
  String get conversationMicrophoneDenied =>
      'Kann nicht auf das Mikrofon zugreifen';

  @override
  String get conversationRecordingFailed => 'Spracherkennung fehlgeschlagen';

  @override
  String get conversationSpeechDisabled => 'Spracheingabe ist deaktiviert';

  @override
  String get conversationSpeechMissing =>
      'Laden Sie das Sprachmodell zuerst in den Einstellungen herunter';

  @override
  String get conversationRecording => 'Aufnahme läuft';

  @override
  String get conversationTranscribing => 'Transkription';

  @override
  String get conversationRecordReady => 'Bereit zum Aufnehmen';

  @override
  String get conversationStartRecording => 'Aufnahme starten';

  @override
  String get conversationFinishRecording => 'Aufnahme beenden';

  @override
  String get conversationSources => 'Quellen';

  @override
  String get conversationSearchSuggestions => 'Vorschläge für die Suche';

  @override
  String get conversationStats => 'Nutzung des Chats';

  @override
  String get conversationStatsEmpty => 'Noch keine Nutzung';

  @override
  String get conversationStatsOverview => 'Übersicht';

  @override
  String get conversationStatsTokenGroup => 'Token-Nutzung';

  @override
  String get conversationStatsCostGroup => 'Kosten';

  @override
  String get conversationStatsGenerationGroup => 'Generierung';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Eingabe';

  @override
  String get conversationStatsOutput => 'Ausgabe';

  @override
  String get conversationStatsCached => 'Zwischengespeicherte Eingabe';

  @override
  String get conversationStatsReasoning => 'Argumentationsausgang';

  @override
  String get conversationStatsCacheRate => 'Cache-Treffer';

  @override
  String get conversationStatsCost => 'Geschätzte Kosten';

  @override
  String get conversationStatsCostCoverage => 'Kostendeckung';

  @override
  String get conversationStatsSpeed => 'Generationsgeschwindigkeit';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Zeitabdeckung';

  @override
  String get conversationStatsTurns => 'Runden';

  @override
  String get conversationStatsResponses => 'Modellantworten';

  @override
  String get conversationStatsContext => 'Der aktuelle Kontext';

  @override
  String get conversationStatsInputCost => 'Inputkosten';

  @override
  String get conversationStatsOutputCost => 'Outputkosten';

  @override
  String get conversationStatsCacheReadCost => 'Cache-Lesekosten';

  @override
  String get conversationStatsCacheWriteCost => 'Cache-Schreibkosten';

  @override
  String get messageHistoryUpdated =>
      'Chat aktualisiert; ursprüngliche Aufzeichnungen erhalten';

  @override
  String get turnUndoUnsaved =>
      'Dateien haben nicht gespeicherte Änderungen; speichern oder verwerfen Sie sie zuerst';

  @override
  String get codePlain => 'Klartext';

  @override
  String get toolArguments => 'Argumente';

  @override
  String get toolResult => 'Ergebnis';

  @override
  String get toolRaw => 'Rohergebnis';

  @override
  String get turnChanges => 'Wenden Sie Änderungen';

  @override
  String turnChangesCount(String count) {
    return '$count-Dateien';
  }

  @override
  String get turnUndo => 'Änderungen rückgängig machen';

  @override
  String get turnUndoAll => 'Alles rückgängig';

  @override
  String get turnUndoConfirm =>
      'Diese Dateiänderungen ab diesem Zug rückgängig machen? Konflikte mit späteren Änderungen stoppen den Vorgang';

  @override
  String get turnUndoDone => 'Rückgängig gemacht';

  @override
  String get turnUndoPartial =>
      'Einige Änderungen rückgängig gemacht; Überprüfen der verbleibenden Dateien';

  @override
  String get messageActions => 'Aktionen für Nachrichten';

  @override
  String get messageEdit => 'Bearbeiten und regenerieren';

  @override
  String get messageEditConfirm =>
      'Diese Nachricht und den folgenden Chat ersetzen? Dateien werden nicht zurückgesetzt';

  @override
  String get messageRewind => 'Hier zurückspulen';

  @override
  String get messageRewindConfirm =>
      'Spätere Chat-Aufzeichnungen werden gesichert; Dateien werden nicht zurückgeschaltet, wenn sie nicht mehr benötigt werden';

  @override
  String get messageBackup => 'Chat-Backup anzeigen';

  @override
  String get messageRegenerate => 'Neu generieren';

  @override
  String get messageSearch => 'Chat suchen';

  @override
  String get messageSearchHint => 'Nachrichten durchsuchen';

  @override
  String get messageSearchMissing =>
      'Diese Nachricht ist nicht mehr im aktuellen Chat';

  @override
  String get messageSearchStale => 'Chat geändert; erneut suchen';

  @override
  String get messageNoResults => 'Keine passenden Nachrichten';

  @override
  String get messageCheck => 'Operationsergebnis prüfen';

  @override
  String get messageReference => 'Referenz';

  @override
  String get messageReferenceContext =>
      'Diese Referenz gehört zum Kontext, in dem die Nachricht gesendet wurde';

  @override
  String get toolFailed => 'Fehlgeschlagen';

  @override
  String toolExitCode(String code) {
    return 'Ausgangscode $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Abgeschlossen durch Signal $signal';
  }

  @override
  String get toolTimedOut => 'Befehl abgelaufen';

  @override
  String get toolCancelled => 'Befehl abgebrochen';

  @override
  String get toolOutcomeUnknown => 'Befehlergebnis unbekannt';

  @override
  String get toolQuestionAnswered => 'Beantwortet';

  @override
  String get toolQuestionDeclined => 'Abgelehnt';

  @override
  String get toolQuestionCancelled => 'Abgebrochen';

  @override
  String get fileLinkUnavailable => 'Dieser Link kann nicht geöffnet werden';

  @override
  String get imagePreview => 'Vorschau des Bildes';

  @override
  String get fileOpenExternal => 'Mit einer anderen App öffnen';

  @override
  String get fileOpenFailed => 'Datei nicht öffnen';

  @override
  String get fileNoApplication => 'Keine App kann diese Datei öffnen';

  @override
  String get fileSaveBeforeShare => 'Änderungen vor dem Teilen speichern?';

  @override
  String fileTrashConfirm(String name) {
    return '“$name” in den Papierkorb des Hosts verschieben? Nicht gespeicherte Änderungen werden ebenfalls verworfen';
  }

  @override
  String get fileTrashUncertain =>
      'Löschungsergebnis nicht bestätigt; die Abfrage erneut versuchen';

  @override
  String get fileSaveFailed => 'Dateispeicher fehlgeschlagen';

  @override
  String get terminalHideKeyboard => 'Tastatur ausblenden';

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
  String get terminalArrowLeft => 'Links';

  @override
  String get terminalArrowUp => 'Nach oben';

  @override
  String get terminalArrowDown => 'Nach unten';

  @override
  String get terminalArrowRight => 'Rechts';

  @override
  String get resourceNoWorkspace =>
      'Auswählen eines Arbeitsbaums auf einem verbundenen Host';

  @override
  String get resourceDisconnected => 'Host nicht verbunden';

  @override
  String get resourceRoot => 'Wurzel';

  @override
  String get resourceMore => 'Mehr laden »';

  @override
  String get resourcePartial => 'Teilweise Inhalte angezeigt';

  @override
  String get resourceEmpty => 'Keine Inhalte gefunden';

  @override
  String get resourceSaveError =>
      'Speichern fehlgeschlagen; Entwurf beibehalten';

  @override
  String get resourceReloadConfirm =>
      'Den Entwurf verwerfen und den neuesten Inhalt laden?';

  @override
  String get resourceWorktreeCreate => 'Neuer Arbeitsbaum';

  @override
  String get resourceSessionServices => 'Chat-Services';

  @override
  String get resourceServicesUnavailable => 'Serviceliste nicht verfügbar';

  @override
  String get resourceNoServices => 'Keine Serviceadressen gefunden';

  @override
  String get resourceServiceOpen => 'Offener Service';

  @override
  String get resourceRemotePort => 'Remote Port';

  @override
  String get resourceOpenPort => 'Forward-Port';

  @override
  String get resourceOpenBrowser => 'Vorschau der Webseite';

  @override
  String get resourcePreviewFailed => 'Seite konnte nicht gefunden werden';

  @override
  String get resourcePreviewLink =>
      'Dieser Link kann nicht in der Vorschau geöffnet werden';

  @override
  String get resourceForwardStopped => 'Die Weiterleitung wurde gestoppt';

  @override
  String get resourceTerminalControl => 'Übernehmen Sie die Kontrolle';

  @override
  String get resourceTerminalControlHint => 'Von einem anderen Gerät gesteuert';

  @override
  String get resourceTerminalClaiming => 'Die Kontrolle übernehmen';

  @override
  String get resourceTerminalReadOnly => 'Read-only-Terminal';

  @override
  String get resourceTerminalEnded => 'Terminal beendet';

  @override
  String get resourceTerminalConnecting => 'Anschlussklemme';

  @override
  String get resourceTerminalInput => 'Terminal-Eingang';

  @override
  String get resourceTerminalPaste => 'Einfügen';

  @override
  String get resourceGitNotRepository =>
      'Dieses Verzeichnis ist kein Git-Repository';

  @override
  String get resourceInvalidPort => 'Geben Sie einen Port von 1–65535 ein';

  @override
  String get tool_navigate => 'Seite öffnen';

  @override
  String get tool_back => 'Zurück gehen »';

  @override
  String get tool_forward => 'Gehen Sie vorwärts';

  @override
  String get tool_refresh => 'Seite aktualisieren';

  @override
  String get tool_right_click => 'Klickelement';

  @override
  String get tool_clear => 'Text eingeben';

  @override
  String get tool_select => 'Wählen Sie eine Option';

  @override
  String get tool_hover => 'Hover';

  @override
  String get tool_scroll => 'Seite scrollen';

  @override
  String get tool_press_key => 'Taste drücken';

  @override
  String get tool_new_tab => 'Neue Registerkarte';

  @override
  String get tool_list_windows => 'Browser-Tabs';

  @override
  String get tool_switch_window => 'Schalter';

  @override
  String get tool_close_window => 'Fenster schließen »';

  @override
  String get tool_close_session => 'Schließen Sie den Browser';

  @override
  String get tool_screenshot => 'Seite aufnehmen';

  @override
  String get tool_print_to_pdf => 'Exportieren als PDF';

  @override
  String get tool_file_upload => 'Datei hochladen';

  @override
  String get tool_downloads => 'Alle Downloads anzeigen';

  @override
  String get tool_save_download => 'Heruntergeladene Datei speichern';

  @override
  String get tool_evaluate_js => 'Seitenskript ausführen';

  @override
  String get tool_get_cookies => 'Lesen von Cookies';

  @override
  String get tool_delete_all_cookies => 'Ändern Sie Cookies';

  @override
  String get tool_drag_and_drop => 'Drag-Element';

  @override
  String get tool_focus => 'Fokuselement';

  @override
  String get tool_handle_alert => 'Handle-Seite Alarm';

  @override
  String get tool_database_catalog => 'Datenbank durchsuchen';

  @override
  String get tool_database_query => 'Abfragedatenbank';

  @override
  String get tool_database_execute => 'Datenbankoperation ausführen';

  @override
  String get tool_search_memory => 'Suchspeicher';

  @override
  String get tool_review_memories => 'Überprüfen Sie Erinnerungen';

  @override
  String get tool_consolidate_memories => 'Erinnerungen zusammenführen';

  @override
  String get tool_save_memory => 'Speicher sparen';

  @override
  String get tool_forget_memory => 'Speicher löschen';

  @override
  String get tool_update_plan => 'Update-Plan';

  @override
  String get tool_create_goal => 'Ziel erstellen';

  @override
  String get tool_get_goal => 'Ziel anzeigen';

  @override
  String get tool_update_goal => 'Ziel aktualisieren';

  @override
  String get tool_spawn_agent => 'Unteragent';

  @override
  String get tool_browser_tabs => 'Browser-Tabs';

  @override
  String get tool_browser_read => 'Lesen Sie Seite';

  @override
  String get tool_browser_navigate => 'Seite öffnen';

  @override
  String get tool_browser_click => 'Klickelement';

  @override
  String get tool_browser_input => 'Text eingeben';

  @override
  String get tool_browser_scroll => 'Seite scrollen';

  @override
  String get tool_browser_back => 'Zurück gehen »';

  @override
  String get tool_browser_forward => 'Gehen Sie vorwärts';

  @override
  String get tool_browser_refresh => 'Seite aktualisieren';

  @override
  String get tool_browser_open => 'Neue Registerkarte';

  @override
  String get tool_browser_close => 'Schließen Sie den Tab';

  @override
  String get tool_browser_focus => 'Schalter';

  @override
  String get tool_browser_select => 'Wählen Sie eine Option';

  @override
  String get tool_browser_hover => 'Hover';

  @override
  String get tool_browser_key => 'Taste drücken';

  @override
  String get tool_browser_frame => 'Schalterrahmen';

  @override
  String get tool_browser_wait => 'Warten auf Seite';

  @override
  String get tool_browser_screenshot => 'Seite aufnehmen';

  @override
  String get tool_ssh_run => 'Befehl SSH ausführen';

  @override
  String get tool_ssh_transfer => 'Datei SSH übertragen';

  @override
  String get tool_list_worktrees => 'Liste der Arbeitsbäume';

  @override
  String get tool_create_worktree => 'Neuer Arbeitsbaum';

  @override
  String get tool_register_worktree => 'Worktree hinzufügen';

  @override
  String get tool_remove_worktree => 'Worktree entfernen';

  @override
  String get tool_google_search => 'Web durchsuchen';

  @override
  String get tool_web_fetch => 'Seite abrufen';

  @override
  String get tool_fetch_url => 'Seite abrufen';

  @override
  String get tool_read_file => 'Datei lesen';

  @override
  String get tool_write_file => 'Datei schreiben';

  @override
  String get tool_list_directory => 'Verzeichnis durchsuchen';

  @override
  String get tool_search_files => 'Dateien durchsuchen';

  @override
  String get tool_run_command => 'Befehl ausführen';

  @override
  String get tool_read_command => 'Befehl Hintergrund anzeigen';

  @override
  String get tool_stop_command => 'Stoppbefehl';

  @override
  String get tool_load_skill => 'Load Skill';

  @override
  String get tool_read_skill_resource => 'Skill-Ressource lesen';

  @override
  String get tool_computer_desktop => 'Desktop anzeigen';

  @override
  String get tool_computer_observe => 'Bildschirm beobachten';

  @override
  String get tool_computer_input => 'Steuerrechner';

  @override
  String get tool_computer_focus => 'Die Switch-App';

  @override
  String get tool_computer_open => 'Die App öffnen';

  @override
  String get tool_git_status => 'Git Status';

  @override
  String get tool_git_diff => 'Diff anzeigen';

  @override
  String get tool_git_log => 'Git log';

  @override
  String get tool_inspect_image => 'Bild inspizieren';

  @override
  String get tool_generate_image => 'Bild generieren';

  @override
  String get tool_generate_video => 'Video generieren';

  @override
  String get terminalUnavailable => 'Terminal ist nicht verbunden';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Aktualisieren';

  @override
  String get loading => 'Wird geladen';

  @override
  String get home => 'Chats';

  @override
  String get idle => 'Inaktiv';

  @override
  String get allProjects => 'Alle Projekte anzeigen';

  @override
  String get allWorktrees => 'Alle Arbeitsbäume anzeigen';

  @override
  String get filterProjects => 'Projekte nach Filtern';

  @override
  String get closeSearch => 'Die Suche schließen';

  @override
  String onlineHostCount(String count) {
    return '$count online';
  }

  @override
  String get taskActions => 'Task-Aktionen';

  @override
  String get archiveShort => 'Archiv';

  @override
  String get archiveTab => 'Archiviert';

  @override
  String get archivedTasks => 'Archiviert';

  @override
  String get delete => 'Löschen';

  @override
  String get deleteTask => 'Chat löschen';

  @override
  String get deleteWarning =>
      'Dieser Chat kann nach dem Löschen nicht fortgesetzt werden';

  @override
  String get busyDelete =>
      'Beenden Sie die Aufgabe, bevor Sie diesen Chat löschen';

  @override
  String get stopBeforeDelete => 'Aufgabe stoppen';

  @override
  String get deleted => 'Chat aus der Vorschau entfernt';

  @override
  String get restored => 'Zu Hause wiederhergestellt';

  @override
  String get restore => 'Wiederherstellen';

  @override
  String get archiveEmpty => 'Keine archivierten Chats';

  @override
  String get archiveKeepsRunning =>
      'Durch die Archivierung werden laufende Aufgaben nicht gestoppt';

  @override
  String get title => 'Sailry · Mobile Vorschau';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Mobiler Arbeitsbereich';

  @override
  String get edition => 'MOBILE ERFORSCHUNG / 01';

  @override
  String get intro => 'Aufgaben, Chats und Remote-Arbeitsbereiche';

  @override
  String get preview => 'Vorschau';

  @override
  String get sample => 'Beispieldaten · Änderungen bleiben auf dieser Seite';

  @override
  String get mixed => 'Hell und dunkel';

  @override
  String get dark => 'Dunkel';

  @override
  String get light => 'Hell';

  @override
  String get gallery => 'Übersicht';

  @override
  String get focus => 'Einzelbildschirm';

  @override
  String get reset => 'Vorschau zurücksetzen';

  @override
  String get page => 'Seite wählen »';

  @override
  String get experience => 'Seite öffnen';

  @override
  String get backGallery => 'Zurück zur Übersicht';

  @override
  String get design => 'Funktionen und Design';

  @override
  String get footer => 'Segelboot / Mobil';

  @override
  String get footerNote => 'Lokale HTML-Vorschau · Keine Dienstverbindung';

  @override
  String get tasks => 'Aufgaben';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Hosts';

  @override
  String get resources => 'Ressource';

  @override
  String get settings => 'Einstellungen';

  @override
  String get usage => 'Nutzung';

  @override
  String get changes => 'Änderungen';

  @override
  String get terminal => 'Terminal';

  @override
  String get newTerminal => 'Das neue Terminal';

  @override
  String get files => 'Dateien';

  @override
  String get project => 'Projekt';

  @override
  String get worktree => 'Worktree';

  @override
  String get subtitleTasks =>
      'Hostübergreifende Aufgaben · Genehmigungen und Antworten zuerst';

  @override
  String get subtitleChat =>
      'Kontinuierlicher Chat · Erweitern Sie die Werkzeugaktivität nach Bedarf';

  @override
  String get subtitleHosts => 'Verbindungen, Hostressourcen und Prozesse';

  @override
  String get subtitleResources => 'Host → Projekt → Arbeitsbaum';

  @override
  String get subtitleChanges => 'Datei-Diffs, Staging und Commits';

  @override
  String get subtitleTerminal =>
      'Remote-Terminal · Explizite Eingangssteuerung';

  @override
  String get subtitleUsage => 'Sailry Chats · Über Hosts hinweg aggregiert';

  @override
  String get subtitleSettings =>
      'Lokale Voreinstellungen und Ausführungseinstellungen Node';

  @override
  String get allHosts => 'Alle Hosts anzeigen';

  @override
  String get connectedHosts => '2 online';

  @override
  String get all => 'Alle';

  @override
  String get running => 'Wird ausgeführt';

  @override
  String get waiting => 'Ausstehend';

  @override
  String get completed => 'Abgeschlossen';

  @override
  String get taskProgress => 'Aktuelle Aufgabe';

  @override
  String get taskWait => 'Wir warten auf Ihre Entscheidung';

  @override
  String get taskRecent => 'Vor kurzem abgeschlossen';

  @override
  String get search => 'Suchen';

  @override
  String get searchTasks => 'Aufgaben und Projekte suchen';

  @override
  String get filterTasks => 'Aufgaben filtern';

  @override
  String get noResults => 'Keine passenden Aufgaben';

  @override
  String get notification => 'Benachrichtigungen';

  @override
  String get newTask => 'Neue Aufgabe';

  @override
  String get newConversation => 'Neuen Chat erstellen';

  @override
  String get approveTitle => 'Aktualisieren Sie das Login-Layout';

  @override
  String get approveNote => 'Projekttests durchführen';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'API-Dokumentation organisieren';

  @override
  String get questionNote => 'Warten auf Antwort';

  @override
  String get question =>
      'In welcher Sprache soll die Dokumentation verwendet werden?';

  @override
  String get optionChinese => 'Chinesisch';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Antwort';

  @override
  String get approval => 'Genehmigung';

  @override
  String get viewRequest => 'Anfrage anzeigen';

  @override
  String get taskSearch => 'Verbessern Sie die Dateisuche';

  @override
  String get taskSearchNote => 'Überprüfen des Verzeichnisindexes';

  @override
  String get taskTest => 'Fix Chat-Wiederherstellung';

  @override
  String get taskTestNote => 'Laufversuche';

  @override
  String get taskDone => 'Aktualisieren Sie das Projekt README';

  @override
  String get taskDoneNote => '3 Dateien geändert';

  @override
  String get ago => 'Gerade jetzt';

  @override
  String get minutesAgo => 'Vor 12 Stunden';

  @override
  String get allow => 'Einmal zulassen';

  @override
  String get deny => 'Ablehnen';

  @override
  String get approved => 'Erlaubt · Probe';

  @override
  String get denied => 'Abgelehnt · Beispiel';

  @override
  String get answered => 'Beantwortet · Beispiel';

  @override
  String get awaiting => 'Warten auf Genehmigung';

  @override
  String get working => 'In Bearbeitung';

  @override
  String get viewChanges => 'Änderungen anzeigen';

  @override
  String get viewConversation => 'Chat anzeigen';

  @override
  String get chatTitle => 'Aktualisieren Sie das Login-Layout';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Heute 09:36';

  @override
  String get userMessage =>
      'Passen Sie den Loginabstand an und vereinheitlichen Sie Eingabe- und Schaltflächenstile unter Beibehaltung der Login-Logik';

  @override
  String get assistantMessage =>
      'Ich habe die Anmeldeseite und die gemeinsamen Formularkomponenten, den einheitlichen Eingabeabstand und die hinzugefügten Tastaturfokusstile überprüft';

  @override
  String get replyPreview => 'Beispielaufgabenablauf';

  @override
  String get phaseThinking => 'Denken';

  @override
  String get phaseReading => 'Lesen von Dateien';

  @override
  String get phaseQuestion => 'Warten auf Antwort';

  @override
  String get phaseEditing => 'Bearbeiten von Dateien';

  @override
  String get phaseApproval => 'Warten auf Genehmigung';

  @override
  String get phaseTesting => 'Laufversuche';

  @override
  String get phaseReply => 'Antworten';

  @override
  String get phaseFollowup => 'Warteschlange wird verarbeitet';

  @override
  String get phaseComplete => 'Abgeschlossen';

  @override
  String get phaseFailed => 'Die Tests sind fehlgeschlagen';

  @override
  String get allowShort => 'Zulassen';

  @override
  String get queueShort => 'Warteschlange';

  @override
  String get confirmShort => 'Bestätigen';

  @override
  String get todoShort => 'Zu tun';

  @override
  String get todoInspect => 'Überprüfen Sie die Login-Seite';

  @override
  String get todoEdit => 'Formularformate anpassen';

  @override
  String get todoTest => 'Projekttests durchführen';

  @override
  String get todoNarrow => 'Überprüfen Sie den schmalen Bildschirmabstand';

  @override
  String get workProcess => 'Aktivität';

  @override
  String workSteps(String count) {
    return '· $count Schritte';
  }

  @override
  String get questionRecord => 'Layout bestätigen';

  @override
  String get answerRecorded => 'Beantwortet';

  @override
  String get playFlow => 'Aufgabe spielen';

  @override
  String get pauseFlow => 'Demo anhalten';

  @override
  String get nextFlow => 'Der nächste Schritt';

  @override
  String get replyingNow => 'Antworten';

  @override
  String get toolReadLabel => 'Lesen';

  @override
  String get toolEditLabel => 'Bearbeiten';

  @override
  String get toolRunLabel => 'Ausführen';

  @override
  String get readGroup => '3 Dateien';

  @override
  String get readFileResult => 'Datei gelesen';

  @override
  String get readFileProgress => 'Datei lesen';

  @override
  String get flowAttachment =>
      'Login-Update: Formularabstand vereinheitlichen, Tastaturfokus-Stile hinzufügen und Login-Logik beibehalten';

  @override
  String get readResult =>
      'Login.tsx und gemeinsam genutzte Formularformate lesen\nDie mobile Buttonbreite unterscheidet sich vom Formular';

  @override
  String get layoutFindings =>
      'Das Anmeldeformular verwendet Desktop-Abstand und der mobile Button füllt seinen Container nicht';

  @override
  String get layoutQuestion =>
      'Soll der mobile Login-Button die Breite ausfüllen?';

  @override
  String get questionPending => 'Wir warten auf Ihre Antwort';

  @override
  String get wideButton => 'Schaltfläche in voller Breite verwenden';

  @override
  String get keepButton => 'Aktuelle Breite beibehalten';

  @override
  String get editPlan =>
      'Ich werde die Login-Logik beibehalten, den Abstand vereinheitlichen und den mobilen Button in voller Breite anzeigen';

  @override
  String get editPlanKeep =>
      'Ich werde die Button-Breite und Login-Logik beibehalten und nur Abstand und Fokus-Stile anpassen';

  @override
  String get editThinking =>
      'Wiederverwendung vorhandener Stilvariablen und Beschränkung von Layoutänderungen auf das Anmeldeformular';

  @override
  String get editResult =>
      'Aktualisiert 3 Dateien\nFokusstile und mobile Layoutregeln hinzugefügt';

  @override
  String get beforeTest =>
      'Layout-Änderungen sind abgeschlossen. Als nächstes werde ich Projekttests durchführen, um Regressionen zu überprüfen';

  @override
  String get testTool => 'Projekttests durchführen';

  @override
  String get testProgress =>
      'Anmeldeformular-Tests werden ausgeführt…\nÜberprüfen von Fokus und Tastaturinteraktion';

  @override
  String get testResult =>
      '12 Tests bestanden\nKeine Login-Logik-Regressionen gefunden';

  @override
  String get testFailure =>
      'Test der Fokusreihenfolge fehlgeschlagen\nErwarteter Fokus auf das Kennwortfeld, aber es blieb auf dem Benutzernamenfeld';

  @override
  String get testFailed => 'Die Tests sind fehlgeschlagen';

  @override
  String get flowResult =>
      'Die Anmeldeabstände und die Fokusstile sind vereinheitlicht, mit einer mobilen Taste in voller Breite.Alle 12 Tests wurden bestanden und die Anmeldelogik ist unverändert';

  @override
  String get queueSample =>
      'Überprüfen Sie den Tastenabstand auch auf schmalen Bildschirmen';

  @override
  String queueCount(String count) {
    return '$count in der Warteschlange stehende Nachrichten';
  }

  @override
  String queuePaused(String count) {
    return 'Warteschlange angehalten · $count';
  }

  @override
  String get pauseQueue => 'Pause Queue';

  @override
  String get resumeQueue => 'Warteschlange fortsetzen';

  @override
  String get sendNext => 'Senden Sie als nächstes';

  @override
  String get enqueue => 'Zur Warteschlange hinzufügen';

  @override
  String get queuedPreview => 'Zur Sample-Warteschlange hinzugefügt';

  @override
  String get moveUp => 'Nach oben bewegen';

  @override
  String get followupThinking =>
      'Überprüfen Sie vorhandene Haltepunkte, um einen konsistenten Tastenabstand auf schmalen Bildschirmen zu bestätigen';

  @override
  String get followupTool => 'Überprüfen Sie schmale Bildschirmstile';

  @override
  String get followupToolResult =>
      '320px und 390px verwenden die gleichen Abstandsregeln';

  @override
  String get followupResult =>
      'Der Abstand zwischen den Tasten auf schmalen Bildschirmen ist konsistent; keine weiteren Änderungen erforderlich';

  @override
  String get deniedResult =>
      'Tests wurden nicht ausgeführt; aktuelle Änderungen werden beibehalten';

  @override
  String get thinkingNow => 'Denken';

  @override
  String get toolsNow => 'Wird ausgeführt';

  @override
  String get toolPending => 'Nicht gestartet';

  @override
  String get thoughtLive =>
      'Überprüfen Sie zuerst die Anmeldeseite und die Formularkomponenten, um Änderungen an Abstand und Fokus zu identifizieren';

  @override
  String get toolsShort => '3 Aktionen';

  @override
  String get thought => 'Denkaufwand';

  @override
  String get thoughtContent =>
      'Vorhandene Formularkomponenten wiederverwenden und nur das Anmelde-Layout und die Fokusstile anpassen';

  @override
  String get toolsComplete => '3 Aktionen abgeschlossen';

  @override
  String get toolRead => 'Login- und Formularkomponenten lesen';

  @override
  String get toolEdit => 'Abstands- und Fokusformate aktualisieren';

  @override
  String get toolDiff => 'Datei-Diffs überprüfen';

  @override
  String get changedFiles => '3 Dateien geändert';

  @override
  String get approvalBody =>
      'Führen Sie Tests in der sailry-web-Arbeitsstruktur in Studio aus';

  @override
  String get approvalResolved => 'Genehmigung beschlossen';

  @override
  String get chatContinue => 'Beschreiben Sie Ihre Aufgabe weiter';

  @override
  String get describeTask => 'Beschreiben Sie Ihre Aufgabe';

  @override
  String get send => 'Senden';

  @override
  String get attach => 'Anhängen';

  @override
  String get voice => 'Spracheingabe';

  @override
  String get voiceNote => 'Diese Vorschau greift nicht auf das Mikrofon zu';

  @override
  String get attachmentNote => 'Beispielanhang hinzugefügt';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Anhang entfernen';

  @override
  String get sentPreview => 'Nur Vorschau, nicht gesendet';

  @override
  String get model => 'Modell';

  @override
  String get modelSource => 'Aktuelle Chat-Konfiguration · Studio';

  @override
  String get copy => 'Kopieren';

  @override
  String get copied => 'Kopiert';

  @override
  String get copyFailed => 'Kopie nicht verfügbar; Text manuell auswählen';

  @override
  String get more => 'Mehr';

  @override
  String get close => 'Schließen';

  @override
  String get back => 'Zurück';

  @override
  String get cancel => 'Abbrechen';

  @override
  String get save => 'Speichern';

  @override
  String get select => 'Auswählen';

  @override
  String get sessionActions => 'Chat-Aktionen';

  @override
  String get queue => 'Message Queue';

  @override
  String get queueEmpty => 'Keine in der Warteschlange stehenden Nachrichten';

  @override
  String get fork => 'Unterhaltung abzweigen';

  @override
  String get forked => 'Beispielgabel erstellt';

  @override
  String get archive => 'Archiv-Chat';

  @override
  String get archived => 'Archiviert in Vorschau';

  @override
  String get stop => 'Aufgabe stoppen';

  @override
  String get stopped => 'Aufgabe in der Vorschau gestoppt';

  @override
  String get stoppedStatus => 'Gestoppt';

  @override
  String get hostSubtitle => 'Ihre Ausführungsknoten';

  @override
  String get pair => 'Host verbinden';

  @override
  String get online => 'Online';

  @override
  String get offline => 'Offline';

  @override
  String get connection => 'Verbindung';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 Farben';

  @override
  String get laptopSystem => 'Zuletzt online vor 1 Stunde';

  @override
  String get statusHealthy => 'Gesund';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Gedächtnis';

  @override
  String get disk => 'Datenträger';

  @override
  String get metrics => 'Ressourcennutzung';

  @override
  String get activity => 'Aktivität';

  @override
  String get lastHour => 'Letzte 60 Minuten';

  @override
  String get sessionCount => 'Chats';

  @override
  String get terminalCount => 'Terminals';

  @override
  String get projectCount => 'Projekte';

  @override
  String get processes => 'Prozesse';

  @override
  String get process => 'Name';

  @override
  String get network => 'Netz';

  @override
  String get details => 'Details';

  @override
  String get manageHost => 'Details zum Host';

  @override
  String get hostProjects => 'Projekte hosten';

  @override
  String get connectionDetails => 'Details zur Verbindung';

  @override
  String get direct => 'Direkt';

  @override
  String get relay => 'Relais';

  @override
  String get latency => 'Latenz';

  @override
  String get hostOffline =>
      'Host offline; zeigt seinen letzten bekannten Zustand an';

  @override
  String get retry => 'Erneut versuchen';

  @override
  String get retryNote => 'Vorschau ist nicht mit einem echten Host verbunden';

  @override
  String get pairTitle => 'Einen Host verbinden';

  @override
  String get pairDescription =>
      'Geben Sie den 6-stelligen Pairing-Code ein, der auf dem Host angezeigt wird';

  @override
  String get pairCode => 'Pairing Code';

  @override
  String get pairHint => 'Der Pairing-Code läuft in 60 Sekunden ab';

  @override
  String get pairDemo => 'Verbindung simulieren';

  @override
  String get pairSuccess => 'Beispielhost hinzugefügt';

  @override
  String get pairInvalid => '6 Ziffern eingeben';

  @override
  String get workspace => 'Arbeitsbereich';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Host auswählen';

  @override
  String get selectProject => 'Wählen Sie ein Projekt';

  @override
  String get selectBranch => 'Wählen Sie Worktree';

  @override
  String get mainBranch => 'Haupt-Worktree';

  @override
  String get featureBranch => 'Login-Layout';

  @override
  String get connectionTools => 'Verbindungen';

  @override
  String get workspaceResources => 'Arbeitsbereich';

  @override
  String get confirm => 'Bestätigen';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Veränderungen, Zweige und Geschichte';

  @override
  String get gitBranches => 'Branches';

  @override
  String get gitHistory => 'Verlauf';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added Zeilen hinzugefügt · $removed entfernt';
  }

  @override
  String get gitActions => 'Git Aktionen';

  @override
  String get gitFetch => 'Fetch';

  @override
  String get gitPull => 'Pull';

  @override
  String get gitPush => 'Push';

  @override
  String get gitCurrent => 'Aktueller Zweig';

  @override
  String get gitCreateBranch => 'Neue Niederlassung';

  @override
  String get gitBranchName => 'Name der Niederlassung';

  @override
  String get gitSwitch => 'Branch wechseln';

  @override
  String get gitMerge => 'Zweig zusammenführen';

  @override
  String get gitDeleteBranch => 'Zweig löschen';

  @override
  String get gitHistoryLayout => 'Anmeldeformular-Abstand anpassen';

  @override
  String get gitHistoryInit => 'Login-Seite initialisieren';

  @override
  String get gitPreview => 'Git Nur Simulation; Repository unverändert';

  @override
  String get gitDirty => 'Aktuelle Änderungen zuerst übertragen';

  @override
  String get gitSwitchNote =>
      'Zweig in diesem Arbeitsbaum wechseln; nur Simulation';

  @override
  String get gitDeleteNote => 'Ausgewählten Zweig löschen; nur Simulation';

  @override
  String get gitInvalidBranch => 'Ungültiger Name oder Zweig existiert bereits';

  @override
  String get review => 'Prüfen';

  @override
  String get browseFiles => 'Worktree durchsuchen';

  @override
  String get reviewFiles => 'Codeänderungen anzeigen';

  @override
  String get selectWorkspace => 'Projekt- und Arbeitsbaum';

  @override
  String get resourceSummary => '2 Chats · 1 Terminal';

  @override
  String get searchFiles => 'Dateien durchsuchen';

  @override
  String get recentFiles => 'Dateien';

  @override
  String get src => 'Quelle';

  @override
  String get folder => 'Ordner';

  @override
  String get modified => 'Geändert';

  @override
  String get filePreview => 'Dateivorschau';

  @override
  String get fileSample => 'Beispieldateiinhalt';

  @override
  String get edit => 'Bearbeiten';

  @override
  String get savePreview => 'In dieser Vorschau gespeicherte Änderungen';

  @override
  String get unsaved => 'Nicht gespeichert';

  @override
  String get discard => 'Änderungen verwerfen';

  @override
  String get discardConfirm =>
      'Nicht gespeicherte Änderungen an dieser Datei verwerfen?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'Mehr Ressourcen anzeigen';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'Datenbank';

  @override
  String get ports => 'Portweiterleitung';

  @override
  String get browser => 'Web-Vorschau';

  @override
  String get portsSub => '1 Stürmer';

  @override
  String get connectionOwner => 'Ausführung Node · Studio-wohnung';

  @override
  String get openTerminal => 'Open Terminal';

  @override
  String get tables => 'Tabellen';

  @override
  String get portNote => 'Sample forward · Kein lokaler Port-Listener';

  @override
  String get portTarget => 'Zielhafen';

  @override
  String get localPort => 'Lokaler Hafen';

  @override
  String get closePort => 'Schließen Sie nach vorne';

  @override
  String get portClosed => 'Probe vorwärts geschlossen';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Arbeitsbaum';

  @override
  String get staged => 'Vorgemerkt';

  @override
  String get diffSummary => '3 Dateien';

  @override
  String get stage => 'Alle Bühnen';

  @override
  String get unstage => 'Vormerkung aufheben';

  @override
  String get commit => 'Commit';

  @override
  String get commitTitle => 'Übernehmen von Änderungen';

  @override
  String get commitMessage => 'Commit-Meldung';

  @override
  String get commitPlaceholder => 'Beschreiben Sie die Änderungen';

  @override
  String get commitPreview => 'Commit simulieren';

  @override
  String get committed => 'Beispiel-Commit abgeschlossen';

  @override
  String get noChanges => 'Keine Änderungen zum Commit';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Bearbeiten fortsetzen »';

  @override
  String get diffSelection => 'Geänderte Datei auswählen';

  @override
  String get terminalKeyboard => 'Tastatur';

  @override
  String get terminalEnter => 'Eingabe';

  @override
  String get terminalOutputLabel => 'Terminal-Ausgang';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Schreibgeschützt';

  @override
  String get takeControl => 'Übernehmen Sie die Kontrolle';

  @override
  String get hasControl => 'Input Control';

  @override
  String get releaseControl => 'Steuerung freigeben';

  @override
  String get terminalPlaceholder => 'Geben Sie einen Beispielbefehl ein';

  @override
  String get terminalPreview =>
      'Beispielterminal · Befehle werden nicht ausgeführt';

  @override
  String get terminalOutput =>
      'Befehl in der Vorschau empfangen, nicht ausgeführt';

  @override
  String get terminalControlNote =>
      'Steuerung übernehmen, um Eingabe zu senden; hier simuliert';

  @override
  String get usageSubtitle => 'Sailry nur Chats';

  @override
  String get week => 'In dieser Woche';

  @override
  String get month => 'In diesem Monat';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total Antworten bewertet';
  }

  @override
  String get usageEmpty => 'Keine Nutzungsdaten';

  @override
  String get estimatedCost => 'Geschätzte Kosten';

  @override
  String get costCoverage => '42 / 48 Antworten bewertet';

  @override
  String get partial => 'Teildaten';

  @override
  String get sourcesPartial => '2 / 3 Hosts aktualisiert';

  @override
  String get input => 'Eingabe';

  @override
  String get output => 'Ausgabe';

  @override
  String get cached => 'Cache-Treffer';

  @override
  String get modelUsage => 'Modellverteilung';

  @override
  String get hostUsage => 'Host-Nutzung';

  @override
  String get recentRequests => 'Die letzten Antworten';

  @override
  String get allUsage => 'Details zur Verwendung';

  @override
  String get usageNote =>
      'Die Kosten sind Schätzungen; einige Antworten sind nicht preislich ausgewiesen';

  @override
  String get sourceNote =>
      'Offline-Hosts behalten ihre zuletzt bekannten Daten';

  @override
  String get profileSubtitle => 'Mobiler Controller';

  @override
  String get localSettings => 'Lokale Präferenzen';

  @override
  String get nodeSettings => 'Ausführung Node Einstellungen';

  @override
  String get appearance => 'Darstellung';

  @override
  String get notifications => 'Benachrichtigungen';

  @override
  String get enabled => 'Ein';

  @override
  String get disabled => 'Aus';

  @override
  String get add => 'Hinzufügen';

  @override
  String get configName => 'Name';

  @override
  String get configEndpoint => 'Endpunkt';

  @override
  String get configModels => 'Modelle';

  @override
  String get configInstructions => 'Anweisungen';

  @override
  String get configContent => 'Inhalt';

  @override
  String get configEmpty => 'Keine Einträge gefunden';

  @override
  String get configDuplicate => 'Name existiert bereits';

  @override
  String configDelete(String name) {
    return '“$name” löschen?';
  }

  @override
  String get speechInput => 'Spracheingabe';

  @override
  String get developerInstructions =>
      'Ändern Sie den Code für die Aufgabe und überprüfen Sie das Ergebnis';

  @override
  String get reviewerInstructions =>
      'Codeänderungen überprüfen und Probleme identifizieren';

  @override
  String get projectConventions => 'Projektkonventionen';

  @override
  String get memoryContent => 'Beibehalten des vorhandenen Codestils';

  @override
  String get providers => 'Modelle und Anbieter';

  @override
  String get roles => 'Rollen';

  @override
  String get memorySettings => 'Gedächtnis';

  @override
  String get speech => 'Spracheingabe';

  @override
  String nodeSettingsNote(String host) {
    return 'Konfiguration gespeichert auf $host';
  }

  @override
  String get about => 'Über Sailry';

  @override
  String get aboutBody =>
      'Mobile Interaktionsvorschau, nicht mit Diensten verbunden';

  @override
  String get settingsSaved => 'In dieser Vorschau aktualisierte Einstellungen';

  @override
  String get modelPicker => 'Wählen Sie ein Modell';

  @override
  String get nodeDefaults => 'Node Standards';

  @override
  String get providerNote =>
      'Beispielkonfiguration · Anmeldeinformationen bleiben bei der Ausführung Node';

  @override
  String get roleNote => '· Gilt für neue Chats';

  @override
  String get auto => 'Automatisch';

  @override
  String get manual => 'Fragen Sie jedes Mal';

  @override
  String get notificationsNote => 'Steuert nur Vorschaubenachrichtigungen';

  @override
  String get memoryNote => 'Beispiel Node Speicher';

  @override
  String get speechNote => 'Verwendet die Ausführung Node Sprachkonfiguration';

  @override
  String get newTaskHost => 'Ausführungshost';

  @override
  String get newTaskProject => 'Projekt';

  @override
  String get newTaskWorktree => 'Worktree';

  @override
  String get create => 'Erstellen';

  @override
  String get taskCreated => 'Beispiel-Chat erstellt';

  @override
  String get required => 'Beschreiben Sie zuerst Ihre Aufgabe';

  @override
  String get notificationsEmpty => 'Keine neuen Nachrichten';

  @override
  String get reviewTitle => 'Referenzen zum Design';

  @override
  String get reviewIntro =>
      'Seiten folgen dem aktuellen Quelltext; diese Vorschau stellt keine Akzeptanz mobiler Dienste her';

  @override
  String get reviewConversation => 'Chats, Freigaben, Fragen und Warteschlange';

  @override
  String get reviewConversationText =>
      'Aufgaben behalten Host-, Projekt- und Worktree-Eigentum; erweitern Sie Werkzeugdatensätze und Genehmigungen innerhalb von Chats';

  @override
  String get reviewResources => 'Dateien, Git, Klemmen und Anschlüsse';

  @override
  String get reviewResourcesText =>
      'Dateibearbeitung, Staging, Commits und Ports behalten ihre Einstiegspunkte; Details öffnen sich auf sekundären Seiten';

  @override
  String get reviewHosts => 'Host-Verbindungen und Überwachung';

  @override
  String get reviewHostsText =>
      'Gepaarte Knoten, 6-stellige Codes, Ressourcennutzung und Prozesse; Offline-Zustand wird nicht als Live angezeigt';

  @override
  String get reviewUsage => 'Verwendung und Node Einstellungen';

  @override
  String get reviewUsageText =>
      'Sailry Nur Chats; aggregierte Nutzung bleibt vollständig und Kosten geben Schätzungen und Abdeckung an';

  @override
  String get reviewBoundary => 'Mobile Grenze';

  @override
  String get reviewBoundaryText =>
      'Die mobile Bridge zeigt Verbindungen, Chats, Terminals und Nutzung an; diese Vorschau beginnt mit Nr. Node, Modelle, Pairing, Terminals oder Plugins';

  @override
  String get reviewVisual => 'Visuelle Referenzen';

  @override
  String get reviewVisualText =>
      'Referenz 1: Chat-Hierarchie; Referenz 2: Softcards und Floating-Navigation; Referenz 3: Compact-Monitoring';

  @override
  String get hostConnectPrompt => 'Einen Host verbinden';

  @override
  String get hostDisconnected => 'Verbindung verloren';

  @override
  String get language => 'Sprache';

  @override
  String get languageSystem => 'System';

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
  String get backgroundConnection => 'Im Hintergrund verbunden bleiben';

  @override
  String get backgroundConnectionActive => 'Hostverbindungen aktiv halten';

  @override
  String get backgroundConnectionFailed =>
      'Hintergrundverbindung nicht aktiviert; versuchen Sie es erneut';

  @override
  String get resetReasoning => 'Aufwand zurücksetzen';

  @override
  String get completionAlerts => 'Benachrichtigungen zur Fertigstellung';

  @override
  String get notificationsReadAll => 'Alle als gelesen markieren';

  @override
  String get notificationsOpen => 'Öffnen';

  @override
  String get preferencesFailed =>
      'Einstellungen nicht gespeichert; versuchen Sie es erneut';

  @override
  String get connectFirst => 'Verbinden Sie einen Host, um zu beginnen';

  @override
  String get initializing => 'Wird gestartet';

  @override
  String get startupFailed => 'Start fehlgeschlagen';

  @override
  String get retryConnection => 'Erneut versuchen';

  @override
  String get pairAction => 'Verbinden';

  @override
  String get pairFailed => 'Verbindung fehlgeschlagen; versuchen Sie es erneut';

  @override
  String get pairExpired =>
      'Der Pairing-Code ist abgelaufen; erhalte einen neuen Code';

  @override
  String get pairing => 'Verbindung wird hergestellt';

  @override
  String get hostUnavailable => 'Host nicht verbunden';

  @override
  String get hostMetricsFailed => 'Hoststatus kann nicht gelesen werden';

  @override
  String get hostProcessesEmpty => 'Keine Prozesse';

  @override
  String get hostRegisterProject => 'Projekt hinzufügen';

  @override
  String get hostChooseDirectory => 'Verzeichnis auswählen';

  @override
  String get hostChooseFile => 'Datei auswählen';

  @override
  String get hostParentDirectory => 'Übergeordnetes Verzeichnis';

  @override
  String get hostEmptyDirectory => 'Das Verzeichnis ist leer';

  @override
  String get hostLoadMore => 'Mehr laden »';

  @override
  String get hostProjectName => 'Name des Projekts';

  @override
  String get hostProjectPath => 'Projektpfad auf dem Host';

  @override
  String get hostProjectFailed => 'Projekt kann nicht hinzugefügt werden';

  @override
  String get hostUnknown => 'Keine Daten verfügbar';

  @override
  String get hostRefresh => 'Aktualisieren';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Gedächtnis';

  @override
  String get hostMetricDisk => 'Datenträger';

  @override
  String get failureConflict =>
      'Inhalt geändert; neu laden und erneut versuchen';

  @override
  String get failureUnknown =>
      'Ergebnis nicht bestätigt; überprüfen Sie zuerst den Host-Zustand';

  @override
  String get failureDenied => 'Berechtigung verweigert';

  @override
  String get failureUnavailable => 'Host nicht verbunden';

  @override
  String get failureBusy => 'Bitte versuchen Sie es später noch einmal';

  @override
  String get failureGeneric => 'Operation ist fehlgeschlagen';

  @override
  String get settingsSpeechLanguage => 'Sprache';

  @override
  String get settingsSpeechAuto => 'Automatisch erkennen';

  @override
  String get settingsSpeechChinese => 'Chinesisch';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Sprachmodell bereit';

  @override
  String get settingsSpeechDownload => 'Sprachmodell herunterladen';

  @override
  String get settingsSpeechFailed =>
      'Sprachmodell nicht bereit; versuchen Sie es erneut';

  @override
  String get settingsNoHost => 'Verbinden Sie zuerst einen Host';

  @override
  String get settingsUnavailable => 'Nicht verfügbar';

  @override
  String get settingsLoadFailed => 'Konnte nicht geladen werden';

  @override
  String get settingsSaveFailed =>
      'Speichern fehlgeschlagen; Entwurf beibehalten';

  @override
  String get settingsConflict =>
      'Einstellungen geändert; erneut öffnen und erneut versuchen';

  @override
  String get settingsUnknown =>
      'Ergebnis nicht bestätigt; aktualisieren, um zu überprüfen';

  @override
  String get settingsRetry => 'Erneut versuchen';

  @override
  String get settingsLoading => 'Wird geladen';

  @override
  String get settingsRequired => 'Geben Sie einen Wert ein';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => 'Beschreibung';

  @override
  String get settingsInstructions => 'Anweisungen';

  @override
  String get settingsModels => 'Modell-IDs, eine pro Zeile';

  @override
  String get settingsApi => 'API-Format';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'API-Schlüssel';

  @override
  String get settingsEnabled => 'Aktiviert';

  @override
  String get settingsDriver => 'Dienst';

  @override
  String get settingsModel => 'Modell';

  @override
  String get settingsMemoryAuto => 'Automatisch aufnehmen';

  @override
  String get settingsMemoryBudget => 'Kontextbyte';

  @override
  String get settingsMemoryReview => 'Überprüfungsintervall (Tage)';

  @override
  String get settingsMemoryRecords => 'Speichereinträge';

  @override
  String get settingsMemoryKind => 'Typ';

  @override
  String get settingsMemoryUser => 'Benutzer';

  @override
  String get settingsMemoryFeedback => 'Feedback';

  @override
  String get settingsMemoryProject => 'Projekt';

  @override
  String get settingsMemoryReference => 'Referenz';

  @override
  String get settingsArchived => 'Archiviert';

  @override
  String get settingsEmpty => 'Keine Aufzeichnungen gefunden';

  @override
  String get settingsUsageUnknown => 'Unbekannt';

  @override
  String get settingsUsagePartial => 'Einige Hosts sind nicht verfügbar';

  @override
  String get settingsUsageCache => 'Gecacht';

  @override
  String get settingsUsageInput => 'Nicht gecachte Eingabe';

  @override
  String get settingsUsageOutput => 'Ausgabe';

  @override
  String get settingsUsageDaily => 'Täglich';

  @override
  String get settingsUsageWeekly => 'Wöchentlich';

  @override
  String get settingsUtc => 'UTC';
}
