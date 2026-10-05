import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_zh.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'generated/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('zh'),
    Locale('en'),
  ];

  /// No description provided for @updatesVersion.
  ///
  /// In en, this message translates to:
  /// **'Version'**
  String get updatesVersion;

  /// No description provided for @updatesCheck.
  ///
  /// In en, this message translates to:
  /// **'Check for updates'**
  String get updatesCheck;

  /// No description provided for @updatesChecking.
  ///
  /// In en, this message translates to:
  /// **'Checking'**
  String get updatesChecking;

  /// No description provided for @updatesAvailable.
  ///
  /// In en, this message translates to:
  /// **'Version {version} is available'**
  String updatesAvailable(String version);

  /// No description provided for @updatesDownload.
  ///
  /// In en, this message translates to:
  /// **'Download update'**
  String get updatesDownload;

  /// No description provided for @updatesCurrent.
  ///
  /// In en, this message translates to:
  /// **'You\'re up to date'**
  String get updatesCurrent;

  /// No description provided for @updatesUnpublished.
  ///
  /// In en, this message translates to:
  /// **'No mobile release yet'**
  String get updatesUnpublished;

  /// No description provided for @updatesCheckFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not check for updates'**
  String get updatesCheckFailed;

  /// No description provided for @updatesOpenFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not open the download'**
  String get updatesOpenFailed;

  /// No description provided for @retryTask.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get retryTask;

  /// No description provided for @welcomeTitle.
  ///
  /// In en, this message translates to:
  /// **'What would you like to work on today?'**
  String get welcomeTitle;

  /// No description provided for @welcomeExplore.
  ///
  /// In en, this message translates to:
  /// **'Explore a project'**
  String get welcomeExplore;

  /// No description provided for @welcomeExploreDetail.
  ///
  /// In en, this message translates to:
  /// **'Understand its structure and entry points'**
  String get welcomeExploreDetail;

  /// No description provided for @welcomeExplorePrompt.
  ///
  /// In en, this message translates to:
  /// **'Help me understand this project, including its key modules and entry points.'**
  String get welcomeExplorePrompt;

  /// No description provided for @welcomeBuild.
  ///
  /// In en, this message translates to:
  /// **'Build an idea'**
  String get welcomeBuild;

  /// No description provided for @welcomeBuildDetail.
  ///
  /// In en, this message translates to:
  /// **'Bring your idea to life'**
  String get welcomeBuildDetail;

  /// No description provided for @welcomeBuildPrompt.
  ///
  /// In en, this message translates to:
  /// **'I want to add a feature to this project. First confirm the requirements with me and outline an implementation plan.'**
  String get welcomeBuildPrompt;

  /// No description provided for @welcomeReview.
  ///
  /// In en, this message translates to:
  /// **'Review changes'**
  String get welcomeReview;

  /// No description provided for @welcomeReviewDetail.
  ///
  /// In en, this message translates to:
  /// **'Check changes and potential issues'**
  String get welcomeReviewDetail;

  /// No description provided for @welcomeReviewPrompt.
  ///
  /// In en, this message translates to:
  /// **'Review the current changes in this project, focusing on potential issues and missing tests.'**
  String get welcomeReviewPrompt;

  /// No description provided for @welcomePlan.
  ///
  /// In en, this message translates to:
  /// **'Make a plan'**
  String get welcomePlan;

  /// No description provided for @welcomePlanDetail.
  ///
  /// In en, this message translates to:
  /// **'Clarify goals and steps'**
  String get welcomePlanDetail;

  /// No description provided for @welcomePlanPrompt.
  ///
  /// In en, this message translates to:
  /// **'Help me create a step-by-step plan for the upcoming development work.'**
  String get welcomePlanPrompt;

  /// No description provided for @conversationEmpty.
  ///
  /// In en, this message translates to:
  /// **'Describe your task'**
  String get conversationEmpty;

  /// No description provided for @conversationLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading chat'**
  String get conversationLoading;

  /// No description provided for @conversationReconnecting.
  ///
  /// In en, this message translates to:
  /// **'Reconnecting'**
  String get conversationReconnecting;

  /// No description provided for @conversationErrorDetails.
  ///
  /// In en, this message translates to:
  /// **'View reason'**
  String get conversationErrorDetails;

  /// No description provided for @conversationModelRetrying.
  ///
  /// In en, this message translates to:
  /// **'Retrying model request {attempt}/{limit}'**
  String conversationModelRetrying(String attempt, String limit);

  /// No description provided for @conversationModelRetried.
  ///
  /// In en, this message translates to:
  /// **'Model request retried {attempt}/{limit}'**
  String conversationModelRetried(String attempt, String limit);

  /// No description provided for @conversationToolsCount.
  ///
  /// In en, this message translates to:
  /// **'{count} tool calls'**
  String conversationToolsCount(String count);

  /// No description provided for @conversationGoal.
  ///
  /// In en, this message translates to:
  /// **'Goal'**
  String get conversationGoal;

  /// No description provided for @conversationGoalBlocked.
  ///
  /// In en, this message translates to:
  /// **'Blocked'**
  String get conversationGoalBlocked;

  /// No description provided for @conversationGoalBudget.
  ///
  /// In en, this message translates to:
  /// **'Budget {count} tokens'**
  String conversationGoalBudget(String count);

  /// No description provided for @conversationStepSkipped.
  ///
  /// In en, this message translates to:
  /// **'Skipped'**
  String get conversationStepSkipped;

  /// No description provided for @conversationChild.
  ///
  /// In en, this message translates to:
  /// **'Subtask'**
  String get conversationChild;

  /// No description provided for @conversationChildReadonly.
  ///
  /// In en, this message translates to:
  /// **'Subtask chat'**
  String get conversationChildReadonly;

  /// No description provided for @conversationOffline.
  ///
  /// In en, this message translates to:
  /// **'Connection lost'**
  String get conversationOffline;

  /// No description provided for @conversationUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Chat unavailable'**
  String get conversationUnavailable;

  /// No description provided for @conversationFailed.
  ///
  /// In en, this message translates to:
  /// **'Operation failed; retry'**
  String get conversationFailed;

  /// No description provided for @conversationUnknown.
  ///
  /// In en, this message translates to:
  /// **'Outcome unconfirmed; check the chat before acting again'**
  String get conversationUnknown;

  /// No description provided for @conversationCheckResult.
  ///
  /// In en, this message translates to:
  /// **'Check outcome'**
  String get conversationCheckResult;

  /// No description provided for @conversationConflict.
  ///
  /// In en, this message translates to:
  /// **'Configuration changed; reopen before acting again'**
  String get conversationConflict;

  /// No description provided for @conversationOlder.
  ///
  /// In en, this message translates to:
  /// **'Load older messages'**
  String get conversationOlder;

  /// No description provided for @conversationNew.
  ///
  /// In en, this message translates to:
  /// **'New chat'**
  String get conversationNew;

  /// No description provided for @conversationNoHost.
  ///
  /// In en, this message translates to:
  /// **'Connect a host first'**
  String get conversationNoHost;

  /// No description provided for @conversationNoProject.
  ///
  /// In en, this message translates to:
  /// **'Add a project on the host first'**
  String get conversationNoProject;

  /// No description provided for @conversationNoModel.
  ///
  /// In en, this message translates to:
  /// **'Configure a model on the host first'**
  String get conversationNoModel;

  /// No description provided for @conversationNoTasks.
  ///
  /// In en, this message translates to:
  /// **'No chats yet'**
  String get conversationNoTasks;

  /// No description provided for @conversationNoMessages.
  ///
  /// In en, this message translates to:
  /// **'No messages'**
  String get conversationNoMessages;

  /// No description provided for @conversationPreviewUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Message unavailable'**
  String get conversationPreviewUnavailable;

  /// No description provided for @conversationInterrupted.
  ///
  /// In en, this message translates to:
  /// **'Interrupted'**
  String get conversationInterrupted;

  /// No description provided for @conversationFailedStatus.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get conversationFailedStatus;

  /// No description provided for @conversationStopping.
  ///
  /// In en, this message translates to:
  /// **'Stopping'**
  String get conversationStopping;

  /// No description provided for @conversationQueued.
  ///
  /// In en, this message translates to:
  /// **'Queued'**
  String get conversationQueued;

  /// No description provided for @conversationProcessing.
  ///
  /// In en, this message translates to:
  /// **'Processing'**
  String get conversationProcessing;

  /// No description provided for @conversationUnsynced.
  ///
  /// In en, this message translates to:
  /// **'State not synced'**
  String get conversationUnsynced;

  /// No description provided for @conversationGenerating.
  ///
  /// In en, this message translates to:
  /// **'Responding'**
  String get conversationGenerating;

  /// No description provided for @conversationWaiting.
  ///
  /// In en, this message translates to:
  /// **'Awaiting confirmation'**
  String get conversationWaiting;

  /// No description provided for @conversationCompacting.
  ///
  /// In en, this message translates to:
  /// **'Compacting context'**
  String get conversationCompacting;

  /// No description provided for @conversationForkConfirm.
  ///
  /// In en, this message translates to:
  /// **'Fork a chat from this record?'**
  String get conversationForkConfirm;

  /// No description provided for @conversationCompacted.
  ///
  /// In en, this message translates to:
  /// **'Context compacted'**
  String get conversationCompacted;

  /// No description provided for @conversationToolWaiting.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get conversationToolWaiting;

  /// No description provided for @conversationToolRunning.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get conversationToolRunning;

  /// No description provided for @conversationToolReturned.
  ///
  /// In en, this message translates to:
  /// **'Returned'**
  String get conversationToolReturned;

  /// No description provided for @conversationToolCancelled.
  ///
  /// In en, this message translates to:
  /// **'Cancelled'**
  String get conversationToolCancelled;

  /// No description provided for @conversationToolNotExecuted.
  ///
  /// In en, this message translates to:
  /// **'Not executed'**
  String get conversationToolNotExecuted;

  /// No description provided for @conversationToolInterrupted.
  ///
  /// In en, this message translates to:
  /// **'Interrupted'**
  String get conversationToolInterrupted;

  /// No description provided for @conversationUnsupportedInput.
  ///
  /// In en, this message translates to:
  /// **'Handle this input on desktop'**
  String get conversationUnsupportedInput;

  /// No description provided for @conversationStartCoding.
  ///
  /// In en, this message translates to:
  /// **'Start execution'**
  String get conversationStartCoding;

  /// No description provided for @conversationPlanFeedback.
  ///
  /// In en, this message translates to:
  /// **'Suggest changes'**
  String get conversationPlanFeedback;

  /// No description provided for @conversationOther.
  ///
  /// In en, this message translates to:
  /// **'Other'**
  String get conversationOther;

  /// No description provided for @conversationSubmit.
  ///
  /// In en, this message translates to:
  /// **'Submit'**
  String get conversationSubmit;

  /// No description provided for @conversationSource.
  ///
  /// In en, this message translates to:
  /// **'Source'**
  String get conversationSource;

  /// No description provided for @conversationMode.
  ///
  /// In en, this message translates to:
  /// **'Work mode'**
  String get conversationMode;

  /// No description provided for @conversationCode.
  ///
  /// In en, this message translates to:
  /// **'Execute'**
  String get conversationCode;

  /// No description provided for @conversationPlan.
  ///
  /// In en, this message translates to:
  /// **'Plan'**
  String get conversationPlan;

  /// No description provided for @conversationPermission.
  ///
  /// In en, this message translates to:
  /// **'Permissions'**
  String get conversationPermission;

  /// No description provided for @conversationAsk.
  ///
  /// In en, this message translates to:
  /// **'Ask each time'**
  String get conversationAsk;

  /// No description provided for @conversationProject.
  ///
  /// In en, this message translates to:
  /// **'Project access'**
  String get conversationProject;

  /// No description provided for @conversationFull.
  ///
  /// In en, this message translates to:
  /// **'Full access'**
  String get conversationFull;

  /// No description provided for @conversationReasoning.
  ///
  /// In en, this message translates to:
  /// **'Reasoning effort'**
  String get conversationReasoning;

  /// No description provided for @conversationDefault.
  ///
  /// In en, this message translates to:
  /// **'Default'**
  String get conversationDefault;

  /// No description provided for @conversationNone.
  ///
  /// In en, this message translates to:
  /// **'Off'**
  String get conversationNone;

  /// No description provided for @conversationMinimal.
  ///
  /// In en, this message translates to:
  /// **'Minimal'**
  String get conversationMinimal;

  /// No description provided for @conversationLow.
  ///
  /// In en, this message translates to:
  /// **'Low'**
  String get conversationLow;

  /// No description provided for @conversationMedium.
  ///
  /// In en, this message translates to:
  /// **'Medium'**
  String get conversationMedium;

  /// No description provided for @conversationHigh.
  ///
  /// In en, this message translates to:
  /// **'High'**
  String get conversationHigh;

  /// No description provided for @conversationXHigh.
  ///
  /// In en, this message translates to:
  /// **'Higher'**
  String get conversationXHigh;

  /// No description provided for @conversationMax.
  ///
  /// In en, this message translates to:
  /// **'Maximum'**
  String get conversationMax;

  /// No description provided for @conversationBudget.
  ///
  /// In en, this message translates to:
  /// **'Reasoning budget'**
  String get conversationBudget;

  /// No description provided for @conversationAttachment.
  ///
  /// In en, this message translates to:
  /// **'Attachment'**
  String get conversationAttachment;

  /// No description provided for @conversationAttachmentTooLarge.
  ///
  /// In en, this message translates to:
  /// **'Attachment unreadable or larger than 64 MB'**
  String get conversationAttachmentTooLarge;

  /// No description provided for @conversationDownload.
  ///
  /// In en, this message translates to:
  /// **'View attachment'**
  String get conversationDownload;

  /// No description provided for @conversationImageFailed.
  ///
  /// In en, this message translates to:
  /// **'Cannot display image'**
  String get conversationImageFailed;

  /// No description provided for @conversationDownloadFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not load attachment'**
  String get conversationDownloadFailed;

  /// No description provided for @conversationReadonly.
  ///
  /// In en, this message translates to:
  /// **'This chat is archived'**
  String get conversationReadonly;

  /// No description provided for @conversationMicrophoneDenied.
  ///
  /// In en, this message translates to:
  /// **'Cannot access microphone'**
  String get conversationMicrophoneDenied;

  /// No description provided for @conversationRecordingFailed.
  ///
  /// In en, this message translates to:
  /// **'Speech recognition failed'**
  String get conversationRecordingFailed;

  /// No description provided for @conversationSpeechDisabled.
  ///
  /// In en, this message translates to:
  /// **'Voice input is off'**
  String get conversationSpeechDisabled;

  /// No description provided for @conversationSpeechMissing.
  ///
  /// In en, this message translates to:
  /// **'Download the speech model in Settings first'**
  String get conversationSpeechMissing;

  /// No description provided for @conversationRecording.
  ///
  /// In en, this message translates to:
  /// **'Recording'**
  String get conversationRecording;

  /// No description provided for @conversationTranscribing.
  ///
  /// In en, this message translates to:
  /// **'Transcribing'**
  String get conversationTranscribing;

  /// No description provided for @conversationRecordReady.
  ///
  /// In en, this message translates to:
  /// **'Ready to record'**
  String get conversationRecordReady;

  /// No description provided for @conversationStartRecording.
  ///
  /// In en, this message translates to:
  /// **'Start recording'**
  String get conversationStartRecording;

  /// No description provided for @conversationFinishRecording.
  ///
  /// In en, this message translates to:
  /// **'Finish recording'**
  String get conversationFinishRecording;

  /// No description provided for @conversationSources.
  ///
  /// In en, this message translates to:
  /// **'Sources'**
  String get conversationSources;

  /// No description provided for @conversationSearchSuggestions.
  ///
  /// In en, this message translates to:
  /// **'Search suggestions'**
  String get conversationSearchSuggestions;

  /// No description provided for @conversationStats.
  ///
  /// In en, this message translates to:
  /// **'Chat usage'**
  String get conversationStats;

  /// No description provided for @conversationStatsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No usage yet'**
  String get conversationStatsEmpty;

  /// No description provided for @conversationStatsOverview.
  ///
  /// In en, this message translates to:
  /// **'Overview'**
  String get conversationStatsOverview;

  /// No description provided for @conversationStatsTokenGroup.
  ///
  /// In en, this message translates to:
  /// **'Token usage'**
  String get conversationStatsTokenGroup;

  /// No description provided for @conversationStatsCostGroup.
  ///
  /// In en, this message translates to:
  /// **'Cost'**
  String get conversationStatsCostGroup;

  /// No description provided for @conversationStatsGenerationGroup.
  ///
  /// In en, this message translates to:
  /// **'Generation'**
  String get conversationStatsGenerationGroup;

  /// No description provided for @conversationStatsTokens.
  ///
  /// In en, this message translates to:
  /// **'Tokens'**
  String get conversationStatsTokens;

  /// No description provided for @conversationStatsInput.
  ///
  /// In en, this message translates to:
  /// **'Input'**
  String get conversationStatsInput;

  /// No description provided for @conversationStatsOutput.
  ///
  /// In en, this message translates to:
  /// **'Output'**
  String get conversationStatsOutput;

  /// No description provided for @conversationStatsCached.
  ///
  /// In en, this message translates to:
  /// **'Cached input'**
  String get conversationStatsCached;

  /// No description provided for @conversationStatsReasoning.
  ///
  /// In en, this message translates to:
  /// **'Reasoning output'**
  String get conversationStatsReasoning;

  /// No description provided for @conversationStatsCacheRate.
  ///
  /// In en, this message translates to:
  /// **'Cache hits'**
  String get conversationStatsCacheRate;

  /// No description provided for @conversationStatsCost.
  ///
  /// In en, this message translates to:
  /// **'Estimated cost'**
  String get conversationStatsCost;

  /// No description provided for @conversationStatsCostCoverage.
  ///
  /// In en, this message translates to:
  /// **'Cost coverage'**
  String get conversationStatsCostCoverage;

  /// No description provided for @conversationStatsSpeed.
  ///
  /// In en, this message translates to:
  /// **'Generation speed'**
  String get conversationStatsSpeed;

  /// No description provided for @conversationStatsSpeedValue.
  ///
  /// In en, this message translates to:
  /// **'{value} tok/s'**
  String conversationStatsSpeedValue(String value);

  /// No description provided for @conversationStatsTimingCoverage.
  ///
  /// In en, this message translates to:
  /// **'Timing coverage'**
  String get conversationStatsTimingCoverage;

  /// No description provided for @conversationStatsTurns.
  ///
  /// In en, this message translates to:
  /// **'Turns'**
  String get conversationStatsTurns;

  /// No description provided for @conversationStatsResponses.
  ///
  /// In en, this message translates to:
  /// **'Model responses'**
  String get conversationStatsResponses;

  /// No description provided for @conversationStatsContext.
  ///
  /// In en, this message translates to:
  /// **'Current context'**
  String get conversationStatsContext;

  /// No description provided for @conversationStatsInputCost.
  ///
  /// In en, this message translates to:
  /// **'Input cost'**
  String get conversationStatsInputCost;

  /// No description provided for @conversationStatsOutputCost.
  ///
  /// In en, this message translates to:
  /// **'Output cost'**
  String get conversationStatsOutputCost;

  /// No description provided for @conversationStatsCacheReadCost.
  ///
  /// In en, this message translates to:
  /// **'Cache read cost'**
  String get conversationStatsCacheReadCost;

  /// No description provided for @conversationStatsCacheWriteCost.
  ///
  /// In en, this message translates to:
  /// **'Cache write cost'**
  String get conversationStatsCacheWriteCost;

  /// No description provided for @messageHistoryUpdated.
  ///
  /// In en, this message translates to:
  /// **'Chat updated; original records kept'**
  String get messageHistoryUpdated;

  /// No description provided for @turnUndoUnsaved.
  ///
  /// In en, this message translates to:
  /// **'Files have unsaved changes; save or discard them first'**
  String get turnUndoUnsaved;

  /// No description provided for @codePlain.
  ///
  /// In en, this message translates to:
  /// **'Plain text'**
  String get codePlain;

  /// No description provided for @toolArguments.
  ///
  /// In en, this message translates to:
  /// **'Arguments'**
  String get toolArguments;

  /// No description provided for @toolResult.
  ///
  /// In en, this message translates to:
  /// **'Result'**
  String get toolResult;

  /// No description provided for @toolRaw.
  ///
  /// In en, this message translates to:
  /// **'Raw result'**
  String get toolRaw;

  /// No description provided for @turnChanges.
  ///
  /// In en, this message translates to:
  /// **'Turn changes'**
  String get turnChanges;

  /// No description provided for @turnChangesCount.
  ///
  /// In en, this message translates to:
  /// **'{count} files'**
  String turnChangesCount(String count);

  /// No description provided for @turnUndo.
  ///
  /// In en, this message translates to:
  /// **'Undo changes'**
  String get turnUndo;

  /// No description provided for @turnUndoAll.
  ///
  /// In en, this message translates to:
  /// **'Undo all'**
  String get turnUndoAll;

  /// No description provided for @turnUndoConfirm.
  ///
  /// In en, this message translates to:
  /// **'Undo these file changes from this turn? Conflicts with later changes will stop the operation'**
  String get turnUndoConfirm;

  /// No description provided for @turnUndoDone.
  ///
  /// In en, this message translates to:
  /// **'Undone'**
  String get turnUndoDone;

  /// No description provided for @turnUndoPartial.
  ///
  /// In en, this message translates to:
  /// **'Some changes undone; check the remaining files'**
  String get turnUndoPartial;

  /// No description provided for @messageActions.
  ///
  /// In en, this message translates to:
  /// **'Message actions'**
  String get messageActions;

  /// No description provided for @messageEdit.
  ///
  /// In en, this message translates to:
  /// **'Edit and regenerate'**
  String get messageEdit;

  /// No description provided for @messageEditConfirm.
  ///
  /// In en, this message translates to:
  /// **'Replace this message and the following chat? Files will not be rolled back'**
  String get messageEditConfirm;

  /// No description provided for @messageRewind.
  ///
  /// In en, this message translates to:
  /// **'Rewind here'**
  String get messageRewind;

  /// No description provided for @messageRewindConfirm.
  ///
  /// In en, this message translates to:
  /// **'Rewind to this turn? Later chat records will be backed up; files will not be rolled back'**
  String get messageRewindConfirm;

  /// No description provided for @messageBackup.
  ///
  /// In en, this message translates to:
  /// **'View chat backup'**
  String get messageBackup;

  /// No description provided for @messageRegenerate.
  ///
  /// In en, this message translates to:
  /// **'Regenerate'**
  String get messageRegenerate;

  /// No description provided for @messageSearch.
  ///
  /// In en, this message translates to:
  /// **'Search chat'**
  String get messageSearch;

  /// No description provided for @messageSearchHint.
  ///
  /// In en, this message translates to:
  /// **'Search messages'**
  String get messageSearchHint;

  /// No description provided for @messageSearchMissing.
  ///
  /// In en, this message translates to:
  /// **'This message is no longer in the current chat'**
  String get messageSearchMissing;

  /// No description provided for @messageSearchStale.
  ///
  /// In en, this message translates to:
  /// **'Chat changed; search again'**
  String get messageSearchStale;

  /// No description provided for @messageNoResults.
  ///
  /// In en, this message translates to:
  /// **'No matching messages'**
  String get messageNoResults;

  /// No description provided for @messageCheck.
  ///
  /// In en, this message translates to:
  /// **'Check operation outcome'**
  String get messageCheck;

  /// No description provided for @messageReference.
  ///
  /// In en, this message translates to:
  /// **'Reference'**
  String get messageReference;

  /// No description provided for @messageReferenceContext.
  ///
  /// In en, this message translates to:
  /// **'This reference belongs to the context when the message was sent'**
  String get messageReferenceContext;

  /// No description provided for @toolFailed.
  ///
  /// In en, this message translates to:
  /// **'Failed'**
  String get toolFailed;

  /// No description provided for @toolExitCode.
  ///
  /// In en, this message translates to:
  /// **'Exit code {code}'**
  String toolExitCode(String code);

  /// No description provided for @toolSignal.
  ///
  /// In en, this message translates to:
  /// **'Terminated by signal {signal}'**
  String toolSignal(String signal);

  /// No description provided for @toolTimedOut.
  ///
  /// In en, this message translates to:
  /// **'Command timed out'**
  String get toolTimedOut;

  /// No description provided for @toolCancelled.
  ///
  /// In en, this message translates to:
  /// **'Command cancelled'**
  String get toolCancelled;

  /// No description provided for @toolOutcomeUnknown.
  ///
  /// In en, this message translates to:
  /// **'Command outcome unknown'**
  String get toolOutcomeUnknown;

  /// No description provided for @toolQuestionAnswered.
  ///
  /// In en, this message translates to:
  /// **'Answered'**
  String get toolQuestionAnswered;

  /// No description provided for @toolQuestionDeclined.
  ///
  /// In en, this message translates to:
  /// **'Declined'**
  String get toolQuestionDeclined;

  /// No description provided for @toolQuestionCancelled.
  ///
  /// In en, this message translates to:
  /// **'Cancelled'**
  String get toolQuestionCancelled;

  /// No description provided for @fileLinkUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Cannot open this link'**
  String get fileLinkUnavailable;

  /// No description provided for @imagePreview.
  ///
  /// In en, this message translates to:
  /// **'Image preview'**
  String get imagePreview;

  /// No description provided for @fileOpenExternal.
  ///
  /// In en, this message translates to:
  /// **'Open with another app'**
  String get fileOpenExternal;

  /// No description provided for @fileOpenFailed.
  ///
  /// In en, this message translates to:
  /// **'Cannot open file'**
  String get fileOpenFailed;

  /// No description provided for @fileNoApplication.
  ///
  /// In en, this message translates to:
  /// **'No app can open this file'**
  String get fileNoApplication;

  /// No description provided for @fileSaveBeforeShare.
  ///
  /// In en, this message translates to:
  /// **'Save changes before sharing?'**
  String get fileSaveBeforeShare;

  /// No description provided for @fileTrashConfirm.
  ///
  /// In en, this message translates to:
  /// **'Move “{name}” to the host trash? Unsaved changes will also be discarded'**
  String fileTrashConfirm(String name);

  /// No description provided for @fileTrashUncertain.
  ///
  /// In en, this message translates to:
  /// **'Deletion outcome unconfirmed; retry the query'**
  String get fileTrashUncertain;

  /// No description provided for @fileSaveFailed.
  ///
  /// In en, this message translates to:
  /// **'File save failed'**
  String get fileSaveFailed;

  /// No description provided for @terminalHideKeyboard.
  ///
  /// In en, this message translates to:
  /// **'Hide keyboard'**
  String get terminalHideKeyboard;

  /// No description provided for @terminalEscape.
  ///
  /// In en, this message translates to:
  /// **'Esc'**
  String get terminalEscape;

  /// No description provided for @terminalTab.
  ///
  /// In en, this message translates to:
  /// **'Tab'**
  String get terminalTab;

  /// No description provided for @terminalCtrl.
  ///
  /// In en, this message translates to:
  /// **'Ctrl'**
  String get terminalCtrl;

  /// No description provided for @terminalAlt.
  ///
  /// In en, this message translates to:
  /// **'Alt'**
  String get terminalAlt;

  /// No description provided for @terminalShift.
  ///
  /// In en, this message translates to:
  /// **'Shift'**
  String get terminalShift;

  /// No description provided for @terminalCmd.
  ///
  /// In en, this message translates to:
  /// **'Cmd'**
  String get terminalCmd;

  /// No description provided for @terminalArrowLeft.
  ///
  /// In en, this message translates to:
  /// **'Left'**
  String get terminalArrowLeft;

  /// No description provided for @terminalArrowUp.
  ///
  /// In en, this message translates to:
  /// **'Up'**
  String get terminalArrowUp;

  /// No description provided for @terminalArrowDown.
  ///
  /// In en, this message translates to:
  /// **'Down'**
  String get terminalArrowDown;

  /// No description provided for @terminalArrowRight.
  ///
  /// In en, this message translates to:
  /// **'Right'**
  String get terminalArrowRight;

  /// No description provided for @resourceNoWorkspace.
  ///
  /// In en, this message translates to:
  /// **'Choose a worktree on a connected host'**
  String get resourceNoWorkspace;

  /// No description provided for @resourceDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Host not connected'**
  String get resourceDisconnected;

  /// No description provided for @resourceRoot.
  ///
  /// In en, this message translates to:
  /// **'Root'**
  String get resourceRoot;

  /// No description provided for @resourceMore.
  ///
  /// In en, this message translates to:
  /// **'Load more'**
  String get resourceMore;

  /// No description provided for @resourcePartial.
  ///
  /// In en, this message translates to:
  /// **'Partial content shown'**
  String get resourcePartial;

  /// No description provided for @resourceEmpty.
  ///
  /// In en, this message translates to:
  /// **'No content'**
  String get resourceEmpty;

  /// No description provided for @resourceSaveError.
  ///
  /// In en, this message translates to:
  /// **'Save failed; draft kept'**
  String get resourceSaveError;

  /// No description provided for @resourceReloadConfirm.
  ///
  /// In en, this message translates to:
  /// **'Discard the draft and load the latest content?'**
  String get resourceReloadConfirm;

  /// No description provided for @resourceWorktreeCreate.
  ///
  /// In en, this message translates to:
  /// **'New worktree'**
  String get resourceWorktreeCreate;

  /// No description provided for @resourceSessionServices.
  ///
  /// In en, this message translates to:
  /// **'Chat services'**
  String get resourceSessionServices;

  /// No description provided for @resourceServicesUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Service list unavailable'**
  String get resourceServicesUnavailable;

  /// No description provided for @resourceNoServices.
  ///
  /// In en, this message translates to:
  /// **'No service addresses found'**
  String get resourceNoServices;

  /// No description provided for @resourceServiceOpen.
  ///
  /// In en, this message translates to:
  /// **'Open service'**
  String get resourceServiceOpen;

  /// No description provided for @resourceRemotePort.
  ///
  /// In en, this message translates to:
  /// **'Remote port'**
  String get resourceRemotePort;

  /// No description provided for @resourceOpenPort.
  ///
  /// In en, this message translates to:
  /// **'Forward port'**
  String get resourceOpenPort;

  /// No description provided for @resourceOpenBrowser.
  ///
  /// In en, this message translates to:
  /// **'Preview web page'**
  String get resourceOpenBrowser;

  /// No description provided for @resourcePreviewFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not load page'**
  String get resourcePreviewFailed;

  /// No description provided for @resourcePreviewLink.
  ///
  /// In en, this message translates to:
  /// **'Cannot open this link in preview'**
  String get resourcePreviewLink;

  /// No description provided for @resourceForwardStopped.
  ///
  /// In en, this message translates to:
  /// **'Forwarding stopped'**
  String get resourceForwardStopped;

  /// No description provided for @resourceTerminalControl.
  ///
  /// In en, this message translates to:
  /// **'Take control'**
  String get resourceTerminalControl;

  /// No description provided for @resourceTerminalControlHint.
  ///
  /// In en, this message translates to:
  /// **'Controlled by another device'**
  String get resourceTerminalControlHint;

  /// No description provided for @resourceTerminalClaiming.
  ///
  /// In en, this message translates to:
  /// **'Taking control'**
  String get resourceTerminalClaiming;

  /// No description provided for @resourceTerminalReadOnly.
  ///
  /// In en, this message translates to:
  /// **'Read-only terminal'**
  String get resourceTerminalReadOnly;

  /// No description provided for @resourceTerminalEnded.
  ///
  /// In en, this message translates to:
  /// **'Terminal ended'**
  String get resourceTerminalEnded;

  /// No description provided for @resourceTerminalConnecting.
  ///
  /// In en, this message translates to:
  /// **'Connecting terminal'**
  String get resourceTerminalConnecting;

  /// No description provided for @resourceTerminalInput.
  ///
  /// In en, this message translates to:
  /// **'Terminal input'**
  String get resourceTerminalInput;

  /// No description provided for @resourceTerminalPaste.
  ///
  /// In en, this message translates to:
  /// **'Paste'**
  String get resourceTerminalPaste;

  /// No description provided for @resourceGitNotRepository.
  ///
  /// In en, this message translates to:
  /// **'This directory is not a Git repository'**
  String get resourceGitNotRepository;

  /// No description provided for @resourceInvalidPort.
  ///
  /// In en, this message translates to:
  /// **'Enter a port from 1–65535'**
  String get resourceInvalidPort;

  /// No description provided for @tool_navigate.
  ///
  /// In en, this message translates to:
  /// **'Open page'**
  String get tool_navigate;

  /// No description provided for @tool_back.
  ///
  /// In en, this message translates to:
  /// **'Go back'**
  String get tool_back;

  /// No description provided for @tool_forward.
  ///
  /// In en, this message translates to:
  /// **'Go forward'**
  String get tool_forward;

  /// No description provided for @tool_refresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh page'**
  String get tool_refresh;

  /// No description provided for @tool_right_click.
  ///
  /// In en, this message translates to:
  /// **'Click element'**
  String get tool_right_click;

  /// No description provided for @tool_clear.
  ///
  /// In en, this message translates to:
  /// **'Enter text'**
  String get tool_clear;

  /// No description provided for @tool_select.
  ///
  /// In en, this message translates to:
  /// **'Select option'**
  String get tool_select;

  /// No description provided for @tool_hover.
  ///
  /// In en, this message translates to:
  /// **'Hover element'**
  String get tool_hover;

  /// No description provided for @tool_scroll.
  ///
  /// In en, this message translates to:
  /// **'Scroll page'**
  String get tool_scroll;

  /// No description provided for @tool_press_key.
  ///
  /// In en, this message translates to:
  /// **'Press key'**
  String get tool_press_key;

  /// No description provided for @tool_new_tab.
  ///
  /// In en, this message translates to:
  /// **'New tab'**
  String get tool_new_tab;

  /// No description provided for @tool_list_windows.
  ///
  /// In en, this message translates to:
  /// **'Browser tabs'**
  String get tool_list_windows;

  /// No description provided for @tool_switch_window.
  ///
  /// In en, this message translates to:
  /// **'Switch tab'**
  String get tool_switch_window;

  /// No description provided for @tool_close_window.
  ///
  /// In en, this message translates to:
  /// **'Close window'**
  String get tool_close_window;

  /// No description provided for @tool_close_session.
  ///
  /// In en, this message translates to:
  /// **'Close browser'**
  String get tool_close_session;

  /// No description provided for @tool_screenshot.
  ///
  /// In en, this message translates to:
  /// **'Capture page'**
  String get tool_screenshot;

  /// No description provided for @tool_print_to_pdf.
  ///
  /// In en, this message translates to:
  /// **'Export PDF'**
  String get tool_print_to_pdf;

  /// No description provided for @tool_file_upload.
  ///
  /// In en, this message translates to:
  /// **'Upload file'**
  String get tool_file_upload;

  /// No description provided for @tool_downloads.
  ///
  /// In en, this message translates to:
  /// **'View downloads'**
  String get tool_downloads;

  /// No description provided for @tool_save_download.
  ///
  /// In en, this message translates to:
  /// **'Save downloaded file'**
  String get tool_save_download;

  /// No description provided for @tool_evaluate_js.
  ///
  /// In en, this message translates to:
  /// **'Run page script'**
  String get tool_evaluate_js;

  /// No description provided for @tool_get_cookies.
  ///
  /// In en, this message translates to:
  /// **'Read cookies'**
  String get tool_get_cookies;

  /// No description provided for @tool_delete_all_cookies.
  ///
  /// In en, this message translates to:
  /// **'Change cookies'**
  String get tool_delete_all_cookies;

  /// No description provided for @tool_drag_and_drop.
  ///
  /// In en, this message translates to:
  /// **'Drag element'**
  String get tool_drag_and_drop;

  /// No description provided for @tool_focus.
  ///
  /// In en, this message translates to:
  /// **'Focus element'**
  String get tool_focus;

  /// No description provided for @tool_handle_alert.
  ///
  /// In en, this message translates to:
  /// **'Handle page alert'**
  String get tool_handle_alert;

  /// No description provided for @tool_database_catalog.
  ///
  /// In en, this message translates to:
  /// **'Browse database'**
  String get tool_database_catalog;

  /// No description provided for @tool_database_query.
  ///
  /// In en, this message translates to:
  /// **'Query database'**
  String get tool_database_query;

  /// No description provided for @tool_database_execute.
  ///
  /// In en, this message translates to:
  /// **'Execute database operation'**
  String get tool_database_execute;

  /// No description provided for @tool_search_memory.
  ///
  /// In en, this message translates to:
  /// **'Search memory'**
  String get tool_search_memory;

  /// No description provided for @tool_review_memories.
  ///
  /// In en, this message translates to:
  /// **'Review memories'**
  String get tool_review_memories;

  /// No description provided for @tool_consolidate_memories.
  ///
  /// In en, this message translates to:
  /// **'Merge memories'**
  String get tool_consolidate_memories;

  /// No description provided for @tool_save_memory.
  ///
  /// In en, this message translates to:
  /// **'Save memory'**
  String get tool_save_memory;

  /// No description provided for @tool_forget_memory.
  ///
  /// In en, this message translates to:
  /// **'Delete memory'**
  String get tool_forget_memory;

  /// No description provided for @tool_update_plan.
  ///
  /// In en, this message translates to:
  /// **'Update plan'**
  String get tool_update_plan;

  /// No description provided for @tool_create_goal.
  ///
  /// In en, this message translates to:
  /// **'Create goal'**
  String get tool_create_goal;

  /// No description provided for @tool_get_goal.
  ///
  /// In en, this message translates to:
  /// **'View goal'**
  String get tool_get_goal;

  /// No description provided for @tool_update_goal.
  ///
  /// In en, this message translates to:
  /// **'Update goal'**
  String get tool_update_goal;

  /// No description provided for @tool_spawn_agent.
  ///
  /// In en, this message translates to:
  /// **'Subagent'**
  String get tool_spawn_agent;

  /// No description provided for @tool_browser_tabs.
  ///
  /// In en, this message translates to:
  /// **'Browser tabs'**
  String get tool_browser_tabs;

  /// No description provided for @tool_browser_read.
  ///
  /// In en, this message translates to:
  /// **'Read page'**
  String get tool_browser_read;

  /// No description provided for @tool_browser_navigate.
  ///
  /// In en, this message translates to:
  /// **'Open page'**
  String get tool_browser_navigate;

  /// No description provided for @tool_browser_click.
  ///
  /// In en, this message translates to:
  /// **'Click element'**
  String get tool_browser_click;

  /// No description provided for @tool_browser_input.
  ///
  /// In en, this message translates to:
  /// **'Enter text'**
  String get tool_browser_input;

  /// No description provided for @tool_browser_scroll.
  ///
  /// In en, this message translates to:
  /// **'Scroll page'**
  String get tool_browser_scroll;

  /// No description provided for @tool_browser_back.
  ///
  /// In en, this message translates to:
  /// **'Go back'**
  String get tool_browser_back;

  /// No description provided for @tool_browser_forward.
  ///
  /// In en, this message translates to:
  /// **'Go forward'**
  String get tool_browser_forward;

  /// No description provided for @tool_browser_refresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh page'**
  String get tool_browser_refresh;

  /// No description provided for @tool_browser_open.
  ///
  /// In en, this message translates to:
  /// **'New tab'**
  String get tool_browser_open;

  /// No description provided for @tool_browser_close.
  ///
  /// In en, this message translates to:
  /// **'Close tab'**
  String get tool_browser_close;

  /// No description provided for @tool_browser_focus.
  ///
  /// In en, this message translates to:
  /// **'Switch tab'**
  String get tool_browser_focus;

  /// No description provided for @tool_browser_select.
  ///
  /// In en, this message translates to:
  /// **'Select option'**
  String get tool_browser_select;

  /// No description provided for @tool_browser_hover.
  ///
  /// In en, this message translates to:
  /// **'Hover element'**
  String get tool_browser_hover;

  /// No description provided for @tool_browser_key.
  ///
  /// In en, this message translates to:
  /// **'Press key'**
  String get tool_browser_key;

  /// No description provided for @tool_browser_frame.
  ///
  /// In en, this message translates to:
  /// **'Switch frame'**
  String get tool_browser_frame;

  /// No description provided for @tool_browser_wait.
  ///
  /// In en, this message translates to:
  /// **'Wait for page'**
  String get tool_browser_wait;

  /// No description provided for @tool_browser_screenshot.
  ///
  /// In en, this message translates to:
  /// **'Capture page'**
  String get tool_browser_screenshot;

  /// No description provided for @tool_ssh_run.
  ///
  /// In en, this message translates to:
  /// **'Run SSH command'**
  String get tool_ssh_run;

  /// No description provided for @tool_ssh_transfer.
  ///
  /// In en, this message translates to:
  /// **'Transfer SSH file'**
  String get tool_ssh_transfer;

  /// No description provided for @tool_list_worktrees.
  ///
  /// In en, this message translates to:
  /// **'List worktrees'**
  String get tool_list_worktrees;

  /// No description provided for @tool_create_worktree.
  ///
  /// In en, this message translates to:
  /// **'New worktree'**
  String get tool_create_worktree;

  /// No description provided for @tool_register_worktree.
  ///
  /// In en, this message translates to:
  /// **'Add worktree'**
  String get tool_register_worktree;

  /// No description provided for @tool_remove_worktree.
  ///
  /// In en, this message translates to:
  /// **'Remove worktree'**
  String get tool_remove_worktree;

  /// No description provided for @tool_google_search.
  ///
  /// In en, this message translates to:
  /// **'Search web'**
  String get tool_google_search;

  /// No description provided for @tool_web_fetch.
  ///
  /// In en, this message translates to:
  /// **'Fetch page'**
  String get tool_web_fetch;

  /// No description provided for @tool_fetch_url.
  ///
  /// In en, this message translates to:
  /// **'Fetch page'**
  String get tool_fetch_url;

  /// No description provided for @tool_read_file.
  ///
  /// In en, this message translates to:
  /// **'Read file'**
  String get tool_read_file;

  /// No description provided for @tool_write_file.
  ///
  /// In en, this message translates to:
  /// **'Write file'**
  String get tool_write_file;

  /// No description provided for @tool_list_directory.
  ///
  /// In en, this message translates to:
  /// **'Browse directory'**
  String get tool_list_directory;

  /// No description provided for @tool_search_files.
  ///
  /// In en, this message translates to:
  /// **'Search files'**
  String get tool_search_files;

  /// No description provided for @tool_run_command.
  ///
  /// In en, this message translates to:
  /// **'Run command'**
  String get tool_run_command;

  /// No description provided for @tool_read_command.
  ///
  /// In en, this message translates to:
  /// **'View background command'**
  String get tool_read_command;

  /// No description provided for @tool_stop_command.
  ///
  /// In en, this message translates to:
  /// **'Stop command'**
  String get tool_stop_command;

  /// No description provided for @tool_load_skill.
  ///
  /// In en, this message translates to:
  /// **'Load skill'**
  String get tool_load_skill;

  /// No description provided for @tool_read_skill_resource.
  ///
  /// In en, this message translates to:
  /// **'Read skill resource'**
  String get tool_read_skill_resource;

  /// No description provided for @tool_computer_desktop.
  ///
  /// In en, this message translates to:
  /// **'View desktop'**
  String get tool_computer_desktop;

  /// No description provided for @tool_computer_observe.
  ///
  /// In en, this message translates to:
  /// **'Observe screen'**
  String get tool_computer_observe;

  /// No description provided for @tool_computer_input.
  ///
  /// In en, this message translates to:
  /// **'Control computer'**
  String get tool_computer_input;

  /// No description provided for @tool_computer_focus.
  ///
  /// In en, this message translates to:
  /// **'Switch app'**
  String get tool_computer_focus;

  /// No description provided for @tool_computer_open.
  ///
  /// In en, this message translates to:
  /// **'Open app'**
  String get tool_computer_open;

  /// No description provided for @tool_git_status.
  ///
  /// In en, this message translates to:
  /// **'Git status'**
  String get tool_git_status;

  /// No description provided for @tool_git_diff.
  ///
  /// In en, this message translates to:
  /// **'View diff'**
  String get tool_git_diff;

  /// No description provided for @tool_git_log.
  ///
  /// In en, this message translates to:
  /// **'Git log'**
  String get tool_git_log;

  /// No description provided for @tool_inspect_image.
  ///
  /// In en, this message translates to:
  /// **'Inspect image'**
  String get tool_inspect_image;

  /// No description provided for @tool_generate_image.
  ///
  /// In en, this message translates to:
  /// **'Generate image'**
  String get tool_generate_image;

  /// No description provided for @tool_generate_video.
  ///
  /// In en, this message translates to:
  /// **'Generate video'**
  String get tool_generate_video;

  /// No description provided for @terminalUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Terminal is not connected'**
  String get terminalUnavailable;

  /// No description provided for @terminalFixture.
  ///
  /// In en, this message translates to:
  /// **'~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯'**
  String get terminalFixture;

  /// No description provided for @refresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get refresh;

  /// No description provided for @loading.
  ///
  /// In en, this message translates to:
  /// **'Loading'**
  String get loading;

  /// No description provided for @home.
  ///
  /// In en, this message translates to:
  /// **'Chats'**
  String get home;

  /// No description provided for @idle.
  ///
  /// In en, this message translates to:
  /// **'Idle'**
  String get idle;

  /// No description provided for @allProjects.
  ///
  /// In en, this message translates to:
  /// **'All projects'**
  String get allProjects;

  /// No description provided for @allWorktrees.
  ///
  /// In en, this message translates to:
  /// **'All worktrees'**
  String get allWorktrees;

  /// No description provided for @filterProjects.
  ///
  /// In en, this message translates to:
  /// **'Filter projects'**
  String get filterProjects;

  /// No description provided for @closeSearch.
  ///
  /// In en, this message translates to:
  /// **'Close search'**
  String get closeSearch;

  /// No description provided for @onlineHostCount.
  ///
  /// In en, this message translates to:
  /// **'{count} online'**
  String onlineHostCount(String count);

  /// No description provided for @taskActions.
  ///
  /// In en, this message translates to:
  /// **'Task actions'**
  String get taskActions;

  /// No description provided for @archiveShort.
  ///
  /// In en, this message translates to:
  /// **'Archive'**
  String get archiveShort;

  /// No description provided for @archiveTab.
  ///
  /// In en, this message translates to:
  /// **'Archived'**
  String get archiveTab;

  /// No description provided for @archivedTasks.
  ///
  /// In en, this message translates to:
  /// **'Archived'**
  String get archivedTasks;

  /// No description provided for @delete.
  ///
  /// In en, this message translates to:
  /// **'Delete'**
  String get delete;

  /// No description provided for @deleteTask.
  ///
  /// In en, this message translates to:
  /// **'Delete chat'**
  String get deleteTask;

  /// No description provided for @deleteWarning.
  ///
  /// In en, this message translates to:
  /// **'This chat cannot be resumed after deletion'**
  String get deleteWarning;

  /// No description provided for @busyDelete.
  ///
  /// In en, this message translates to:
  /// **'Stop the task before deleting this chat'**
  String get busyDelete;

  /// No description provided for @stopBeforeDelete.
  ///
  /// In en, this message translates to:
  /// **'Stop task'**
  String get stopBeforeDelete;

  /// No description provided for @deleted.
  ///
  /// In en, this message translates to:
  /// **'Chat removed from preview'**
  String get deleted;

  /// No description provided for @restored.
  ///
  /// In en, this message translates to:
  /// **'Restored to home'**
  String get restored;

  /// No description provided for @restore.
  ///
  /// In en, this message translates to:
  /// **'Restore'**
  String get restore;

  /// No description provided for @archiveEmpty.
  ///
  /// In en, this message translates to:
  /// **'No archived chats'**
  String get archiveEmpty;

  /// No description provided for @archiveKeepsRunning.
  ///
  /// In en, this message translates to:
  /// **'Archiving does not stop running tasks'**
  String get archiveKeepsRunning;

  /// No description provided for @title.
  ///
  /// In en, this message translates to:
  /// **'Sailry · Mobile preview'**
  String get title;

  /// No description provided for @brand.
  ///
  /// In en, this message translates to:
  /// **'Sailry'**
  String get brand;

  /// No description provided for @mobile.
  ///
  /// In en, this message translates to:
  /// **'Mobile workspace'**
  String get mobile;

  /// No description provided for @edition.
  ///
  /// In en, this message translates to:
  /// **'MOBILE EXPLORATION / 01'**
  String get edition;

  /// No description provided for @intro.
  ///
  /// In en, this message translates to:
  /// **'Tasks, chats and remote workspaces'**
  String get intro;

  /// No description provided for @preview.
  ///
  /// In en, this message translates to:
  /// **'Preview'**
  String get preview;

  /// No description provided for @sample.
  ///
  /// In en, this message translates to:
  /// **'Sample data · Changes stay on this page'**
  String get sample;

  /// No description provided for @mixed.
  ///
  /// In en, this message translates to:
  /// **'Light and dark'**
  String get mixed;

  /// No description provided for @dark.
  ///
  /// In en, this message translates to:
  /// **'Dark'**
  String get dark;

  /// No description provided for @light.
  ///
  /// In en, this message translates to:
  /// **'Light'**
  String get light;

  /// No description provided for @gallery.
  ///
  /// In en, this message translates to:
  /// **'Overview'**
  String get gallery;

  /// No description provided for @focus.
  ///
  /// In en, this message translates to:
  /// **'Single screen'**
  String get focus;

  /// No description provided for @reset.
  ///
  /// In en, this message translates to:
  /// **'Reset preview'**
  String get reset;

  /// No description provided for @page.
  ///
  /// In en, this message translates to:
  /// **'Choose page'**
  String get page;

  /// No description provided for @experience.
  ///
  /// In en, this message translates to:
  /// **'Open page'**
  String get experience;

  /// No description provided for @backGallery.
  ///
  /// In en, this message translates to:
  /// **'Back to overview'**
  String get backGallery;

  /// No description provided for @design.
  ///
  /// In en, this message translates to:
  /// **'Features and design'**
  String get design;

  /// No description provided for @footer.
  ///
  /// In en, this message translates to:
  /// **'SAILRY / MOBILE'**
  String get footer;

  /// No description provided for @footerNote.
  ///
  /// In en, this message translates to:
  /// **'Local HTML preview · No service connection'**
  String get footerNote;

  /// No description provided for @tasks.
  ///
  /// In en, this message translates to:
  /// **'Tasks'**
  String get tasks;

  /// No description provided for @chat.
  ///
  /// In en, this message translates to:
  /// **'Chat'**
  String get chat;

  /// No description provided for @hosts.
  ///
  /// In en, this message translates to:
  /// **'Hosts'**
  String get hosts;

  /// No description provided for @resources.
  ///
  /// In en, this message translates to:
  /// **'Resources'**
  String get resources;

  /// No description provided for @settings.
  ///
  /// In en, this message translates to:
  /// **'Settings'**
  String get settings;

  /// No description provided for @usage.
  ///
  /// In en, this message translates to:
  /// **'Usage'**
  String get usage;

  /// No description provided for @changes.
  ///
  /// In en, this message translates to:
  /// **'Changes'**
  String get changes;

  /// No description provided for @terminal.
  ///
  /// In en, this message translates to:
  /// **'Terminal'**
  String get terminal;

  /// No description provided for @newTerminal.
  ///
  /// In en, this message translates to:
  /// **'New terminal'**
  String get newTerminal;

  /// No description provided for @files.
  ///
  /// In en, this message translates to:
  /// **'Files'**
  String get files;

  /// No description provided for @project.
  ///
  /// In en, this message translates to:
  /// **'Project'**
  String get project;

  /// No description provided for @worktree.
  ///
  /// In en, this message translates to:
  /// **'Worktree'**
  String get worktree;

  /// No description provided for @subtitleTasks.
  ///
  /// In en, this message translates to:
  /// **'Tasks across hosts · Approvals and replies first'**
  String get subtitleTasks;

  /// No description provided for @subtitleChat.
  ///
  /// In en, this message translates to:
  /// **'Continuous chat · Expand tool activity as needed'**
  String get subtitleChat;

  /// No description provided for @subtitleHosts.
  ///
  /// In en, this message translates to:
  /// **'Connections, host resources and processes'**
  String get subtitleHosts;

  /// No description provided for @subtitleResources.
  ///
  /// In en, this message translates to:
  /// **'Host → Project → Worktree'**
  String get subtitleResources;

  /// No description provided for @subtitleChanges.
  ///
  /// In en, this message translates to:
  /// **'File diffs, staging and commits'**
  String get subtitleChanges;

  /// No description provided for @subtitleTerminal.
  ///
  /// In en, this message translates to:
  /// **'Remote terminal · Explicit input control'**
  String get subtitleTerminal;

  /// No description provided for @subtitleUsage.
  ///
  /// In en, this message translates to:
  /// **'Sailry chats · Aggregated across hosts'**
  String get subtitleUsage;

  /// No description provided for @subtitleSettings.
  ///
  /// In en, this message translates to:
  /// **'Local preferences and execution Node settings'**
  String get subtitleSettings;

  /// No description provided for @allHosts.
  ///
  /// In en, this message translates to:
  /// **'All hosts'**
  String get allHosts;

  /// No description provided for @connectedHosts.
  ///
  /// In en, this message translates to:
  /// **'2 online'**
  String get connectedHosts;

  /// No description provided for @all.
  ///
  /// In en, this message translates to:
  /// **'All'**
  String get all;

  /// No description provided for @running.
  ///
  /// In en, this message translates to:
  /// **'Running'**
  String get running;

  /// No description provided for @waiting.
  ///
  /// In en, this message translates to:
  /// **'Pending'**
  String get waiting;

  /// No description provided for @completed.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get completed;

  /// No description provided for @taskProgress.
  ///
  /// In en, this message translates to:
  /// **'Current task'**
  String get taskProgress;

  /// No description provided for @taskWait.
  ///
  /// In en, this message translates to:
  /// **'Awaiting your decision'**
  String get taskWait;

  /// No description provided for @taskRecent.
  ///
  /// In en, this message translates to:
  /// **'Recently completed'**
  String get taskRecent;

  /// No description provided for @search.
  ///
  /// In en, this message translates to:
  /// **'Search'**
  String get search;

  /// No description provided for @searchTasks.
  ///
  /// In en, this message translates to:
  /// **'Search tasks and projects'**
  String get searchTasks;

  /// No description provided for @filterTasks.
  ///
  /// In en, this message translates to:
  /// **'Filter tasks'**
  String get filterTasks;

  /// No description provided for @noResults.
  ///
  /// In en, this message translates to:
  /// **'No matching tasks'**
  String get noResults;

  /// No description provided for @notification.
  ///
  /// In en, this message translates to:
  /// **'Notifications'**
  String get notification;

  /// No description provided for @newTask.
  ///
  /// In en, this message translates to:
  /// **'New task'**
  String get newTask;

  /// No description provided for @newConversation.
  ///
  /// In en, this message translates to:
  /// **'New chat'**
  String get newConversation;

  /// No description provided for @approveTitle.
  ///
  /// In en, this message translates to:
  /// **'Update the login layout'**
  String get approveTitle;

  /// No description provided for @approveNote.
  ///
  /// In en, this message translates to:
  /// **'Run project tests'**
  String get approveNote;

  /// No description provided for @approveContext.
  ///
  /// In en, this message translates to:
  /// **'sailry-web · feature/sign-in'**
  String get approveContext;

  /// No description provided for @questionTitle.
  ///
  /// In en, this message translates to:
  /// **'Organize API documentation'**
  String get questionTitle;

  /// No description provided for @questionNote.
  ///
  /// In en, this message translates to:
  /// **'Awaiting reply'**
  String get questionNote;

  /// No description provided for @question.
  ///
  /// In en, this message translates to:
  /// **'Which language should the documentation use?'**
  String get question;

  /// No description provided for @optionChinese.
  ///
  /// In en, this message translates to:
  /// **'Chinese'**
  String get optionChinese;

  /// No description provided for @optionEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get optionEnglish;

  /// No description provided for @reply.
  ///
  /// In en, this message translates to:
  /// **'Reply'**
  String get reply;

  /// No description provided for @approval.
  ///
  /// In en, this message translates to:
  /// **'Approval'**
  String get approval;

  /// No description provided for @viewRequest.
  ///
  /// In en, this message translates to:
  /// **'View request'**
  String get viewRequest;

  /// No description provided for @taskSearch.
  ///
  /// In en, this message translates to:
  /// **'Improve file search'**
  String get taskSearch;

  /// No description provided for @taskSearchNote.
  ///
  /// In en, this message translates to:
  /// **'Checking the directory index'**
  String get taskSearchNote;

  /// No description provided for @taskTest.
  ///
  /// In en, this message translates to:
  /// **'Fix chat recovery'**
  String get taskTest;

  /// No description provided for @taskTestNote.
  ///
  /// In en, this message translates to:
  /// **'Running tests'**
  String get taskTestNote;

  /// No description provided for @taskDone.
  ///
  /// In en, this message translates to:
  /// **'Update the project README'**
  String get taskDone;

  /// No description provided for @taskDoneNote.
  ///
  /// In en, this message translates to:
  /// **'3 files changed'**
  String get taskDoneNote;

  /// No description provided for @ago.
  ///
  /// In en, this message translates to:
  /// **'Just now'**
  String get ago;

  /// No description provided for @minutesAgo.
  ///
  /// In en, this message translates to:
  /// **'12 minutes ago'**
  String get minutesAgo;

  /// No description provided for @allow.
  ///
  /// In en, this message translates to:
  /// **'Allow once'**
  String get allow;

  /// No description provided for @deny.
  ///
  /// In en, this message translates to:
  /// **'Deny'**
  String get deny;

  /// No description provided for @approved.
  ///
  /// In en, this message translates to:
  /// **'Allowed · Sample'**
  String get approved;

  /// No description provided for @denied.
  ///
  /// In en, this message translates to:
  /// **'Denied · Sample'**
  String get denied;

  /// No description provided for @answered.
  ///
  /// In en, this message translates to:
  /// **'Replied · Sample'**
  String get answered;

  /// No description provided for @awaiting.
  ///
  /// In en, this message translates to:
  /// **'Awaiting approval'**
  String get awaiting;

  /// No description provided for @working.
  ///
  /// In en, this message translates to:
  /// **'Working'**
  String get working;

  /// No description provided for @viewChanges.
  ///
  /// In en, this message translates to:
  /// **'View changes'**
  String get viewChanges;

  /// No description provided for @viewConversation.
  ///
  /// In en, this message translates to:
  /// **'View chat'**
  String get viewConversation;

  /// No description provided for @chatTitle.
  ///
  /// In en, this message translates to:
  /// **'Update the login layout'**
  String get chatTitle;

  /// No description provided for @chatHost.
  ///
  /// In en, this message translates to:
  /// **'sailry-web · Studio'**
  String get chatHost;

  /// No description provided for @branch.
  ///
  /// In en, this message translates to:
  /// **'feature/sign-in'**
  String get branch;

  /// No description provided for @today.
  ///
  /// In en, this message translates to:
  /// **'Today 09:36'**
  String get today;

  /// No description provided for @userMessage.
  ///
  /// In en, this message translates to:
  /// **'Adjust the login spacing and unify input and button styles while preserving the login logic'**
  String get userMessage;

  /// No description provided for @assistantMessage.
  ///
  /// In en, this message translates to:
  /// **'I checked the login page and shared form components, unified input spacing and added keyboard focus styles'**
  String get assistantMessage;

  /// No description provided for @replyPreview.
  ///
  /// In en, this message translates to:
  /// **'Sample task flow'**
  String get replyPreview;

  /// No description provided for @phaseThinking.
  ///
  /// In en, this message translates to:
  /// **'Thinking'**
  String get phaseThinking;

  /// No description provided for @phaseReading.
  ///
  /// In en, this message translates to:
  /// **'Reading files'**
  String get phaseReading;

  /// No description provided for @phaseQuestion.
  ///
  /// In en, this message translates to:
  /// **'Awaiting reply'**
  String get phaseQuestion;

  /// No description provided for @phaseEditing.
  ///
  /// In en, this message translates to:
  /// **'Editing files'**
  String get phaseEditing;

  /// No description provided for @phaseApproval.
  ///
  /// In en, this message translates to:
  /// **'Awaiting approval'**
  String get phaseApproval;

  /// No description provided for @phaseTesting.
  ///
  /// In en, this message translates to:
  /// **'Running tests'**
  String get phaseTesting;

  /// No description provided for @phaseReply.
  ///
  /// In en, this message translates to:
  /// **'Responding'**
  String get phaseReply;

  /// No description provided for @phaseFollowup.
  ///
  /// In en, this message translates to:
  /// **'Processing queue'**
  String get phaseFollowup;

  /// No description provided for @phaseComplete.
  ///
  /// In en, this message translates to:
  /// **'Completed'**
  String get phaseComplete;

  /// No description provided for @phaseFailed.
  ///
  /// In en, this message translates to:
  /// **'Tests failed'**
  String get phaseFailed;

  /// No description provided for @allowShort.
  ///
  /// In en, this message translates to:
  /// **'Allow'**
  String get allowShort;

  /// No description provided for @queueShort.
  ///
  /// In en, this message translates to:
  /// **'Queue'**
  String get queueShort;

  /// No description provided for @confirmShort.
  ///
  /// In en, this message translates to:
  /// **'Confirm'**
  String get confirmShort;

  /// No description provided for @todoShort.
  ///
  /// In en, this message translates to:
  /// **'To do'**
  String get todoShort;

  /// No description provided for @todoInspect.
  ///
  /// In en, this message translates to:
  /// **'Inspect the login page'**
  String get todoInspect;

  /// No description provided for @todoEdit.
  ///
  /// In en, this message translates to:
  /// **'Adjust form styles'**
  String get todoEdit;

  /// No description provided for @todoTest.
  ///
  /// In en, this message translates to:
  /// **'Run project tests'**
  String get todoTest;

  /// No description provided for @todoNarrow.
  ///
  /// In en, this message translates to:
  /// **'Check narrow-screen spacing'**
  String get todoNarrow;

  /// No description provided for @workProcess.
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get workProcess;

  /// No description provided for @workSteps.
  ///
  /// In en, this message translates to:
  /// **'· {count} steps'**
  String workSteps(String count);

  /// No description provided for @questionRecord.
  ///
  /// In en, this message translates to:
  /// **'Confirm layout'**
  String get questionRecord;

  /// No description provided for @answerRecorded.
  ///
  /// In en, this message translates to:
  /// **'Replied'**
  String get answerRecorded;

  /// No description provided for @playFlow.
  ///
  /// In en, this message translates to:
  /// **'Play task'**
  String get playFlow;

  /// No description provided for @pauseFlow.
  ///
  /// In en, this message translates to:
  /// **'Pause demo'**
  String get pauseFlow;

  /// No description provided for @nextFlow.
  ///
  /// In en, this message translates to:
  /// **'Next step'**
  String get nextFlow;

  /// No description provided for @replyingNow.
  ///
  /// In en, this message translates to:
  /// **'Responding'**
  String get replyingNow;

  /// No description provided for @toolReadLabel.
  ///
  /// In en, this message translates to:
  /// **'Read'**
  String get toolReadLabel;

  /// No description provided for @toolEditLabel.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get toolEditLabel;

  /// No description provided for @toolRunLabel.
  ///
  /// In en, this message translates to:
  /// **'Run'**
  String get toolRunLabel;

  /// No description provided for @readGroup.
  ///
  /// In en, this message translates to:
  /// **'3 files'**
  String get readGroup;

  /// No description provided for @readFileResult.
  ///
  /// In en, this message translates to:
  /// **'File read'**
  String get readFileResult;

  /// No description provided for @readFileProgress.
  ///
  /// In en, this message translates to:
  /// **'Reading file'**
  String get readFileProgress;

  /// No description provided for @flowAttachment.
  ///
  /// In en, this message translates to:
  /// **'Login update: unify form spacing, add keyboard focus styles and preserve login logic'**
  String get flowAttachment;

  /// No description provided for @readResult.
  ///
  /// In en, this message translates to:
  /// **'Read Login.tsx and shared form styles\nThe mobile button width differs from the form'**
  String get readResult;

  /// No description provided for @layoutFindings.
  ///
  /// In en, this message translates to:
  /// **'The login form uses desktop spacing and the mobile button does not fill its container'**
  String get layoutFindings;

  /// No description provided for @layoutQuestion.
  ///
  /// In en, this message translates to:
  /// **'Should the mobile login button fill the width?'**
  String get layoutQuestion;

  /// No description provided for @questionPending.
  ///
  /// In en, this message translates to:
  /// **'Awaiting your reply'**
  String get questionPending;

  /// No description provided for @wideButton.
  ///
  /// In en, this message translates to:
  /// **'Use full-width button'**
  String get wideButton;

  /// No description provided for @keepButton.
  ///
  /// In en, this message translates to:
  /// **'Keep current width'**
  String get keepButton;

  /// No description provided for @editPlan.
  ///
  /// In en, this message translates to:
  /// **'I will preserve login logic, unify spacing and make the mobile button full width'**
  String get editPlan;

  /// No description provided for @editPlanKeep.
  ///
  /// In en, this message translates to:
  /// **'I will keep the button width and login logic, adjusting only spacing and focus styles'**
  String get editPlanKeep;

  /// No description provided for @editThinking.
  ///
  /// In en, this message translates to:
  /// **'Reuse existing style variables and limit layout changes to the login form'**
  String get editThinking;

  /// No description provided for @editResult.
  ///
  /// In en, this message translates to:
  /// **'Updated 3 files\nAdded focus styles and mobile layout rules'**
  String get editResult;

  /// No description provided for @beforeTest.
  ///
  /// In en, this message translates to:
  /// **'Layout changes are complete. Next I will run project tests to check for regressions'**
  String get beforeTest;

  /// No description provided for @testTool.
  ///
  /// In en, this message translates to:
  /// **'Run project tests'**
  String get testTool;

  /// No description provided for @testProgress.
  ///
  /// In en, this message translates to:
  /// **'Running login form tests…\nChecking focus and keyboard interaction'**
  String get testProgress;

  /// No description provided for @testResult.
  ///
  /// In en, this message translates to:
  /// **'12 tests passed\nNo login logic regressions found'**
  String get testResult;

  /// No description provided for @testFailure.
  ///
  /// In en, this message translates to:
  /// **'Focus order test failed\nExpected focus on the password field, but it remained on the username field'**
  String get testFailure;

  /// No description provided for @testFailed.
  ///
  /// In en, this message translates to:
  /// **'Tests failed'**
  String get testFailed;

  /// No description provided for @flowResult.
  ///
  /// In en, this message translates to:
  /// **'Login spacing and focus styles are unified, with a full-width mobile button. All 12 tests passed and login logic is unchanged'**
  String get flowResult;

  /// No description provided for @queueSample.
  ///
  /// In en, this message translates to:
  /// **'Check the button spacing on narrow screens too'**
  String get queueSample;

  /// No description provided for @queueCount.
  ///
  /// In en, this message translates to:
  /// **'{count} queued messages'**
  String queueCount(String count);

  /// No description provided for @queuePaused.
  ///
  /// In en, this message translates to:
  /// **'Queue paused · {count}'**
  String queuePaused(String count);

  /// No description provided for @pauseQueue.
  ///
  /// In en, this message translates to:
  /// **'Pause queue'**
  String get pauseQueue;

  /// No description provided for @resumeQueue.
  ///
  /// In en, this message translates to:
  /// **'Resume queue'**
  String get resumeQueue;

  /// No description provided for @sendNext.
  ///
  /// In en, this message translates to:
  /// **'Send next'**
  String get sendNext;

  /// No description provided for @enqueue.
  ///
  /// In en, this message translates to:
  /// **'Add to queue'**
  String get enqueue;

  /// No description provided for @queuedPreview.
  ///
  /// In en, this message translates to:
  /// **'Added to sample queue'**
  String get queuedPreview;

  /// No description provided for @moveUp.
  ///
  /// In en, this message translates to:
  /// **'Move up'**
  String get moveUp;

  /// No description provided for @followupThinking.
  ///
  /// In en, this message translates to:
  /// **'Check existing breakpoints to confirm consistent button spacing on narrow screens'**
  String get followupThinking;

  /// No description provided for @followupTool.
  ///
  /// In en, this message translates to:
  /// **'Check narrow-screen styles'**
  String get followupTool;

  /// No description provided for @followupToolResult.
  ///
  /// In en, this message translates to:
  /// **'320px and 390px use the same spacing rules'**
  String get followupToolResult;

  /// No description provided for @followupResult.
  ///
  /// In en, this message translates to:
  /// **'Narrow-screen button spacing is consistent; no further changes are needed'**
  String get followupResult;

  /// No description provided for @deniedResult.
  ///
  /// In en, this message translates to:
  /// **'Tests were not run; current changes are preserved'**
  String get deniedResult;

  /// No description provided for @thinkingNow.
  ///
  /// In en, this message translates to:
  /// **'Thinking'**
  String get thinkingNow;

  /// No description provided for @toolsNow.
  ///
  /// In en, this message translates to:
  /// **'Executing'**
  String get toolsNow;

  /// No description provided for @toolPending.
  ///
  /// In en, this message translates to:
  /// **'Not started'**
  String get toolPending;

  /// No description provided for @thoughtLive.
  ///
  /// In en, this message translates to:
  /// **'First inspect the login page and form components to identify spacing and focus changes'**
  String get thoughtLive;

  /// No description provided for @toolsShort.
  ///
  /// In en, this message translates to:
  /// **'3 actions'**
  String get toolsShort;

  /// No description provided for @thought.
  ///
  /// In en, this message translates to:
  /// **'Reasoning'**
  String get thought;

  /// No description provided for @thoughtContent.
  ///
  /// In en, this message translates to:
  /// **'Reuse existing form components and adjust only the login layout and focus styles'**
  String get thoughtContent;

  /// No description provided for @toolsComplete.
  ///
  /// In en, this message translates to:
  /// **'3 actions completed'**
  String get toolsComplete;

  /// No description provided for @toolRead.
  ///
  /// In en, this message translates to:
  /// **'Read login and form components'**
  String get toolRead;

  /// No description provided for @toolEdit.
  ///
  /// In en, this message translates to:
  /// **'Update spacing and focus styles'**
  String get toolEdit;

  /// No description provided for @toolDiff.
  ///
  /// In en, this message translates to:
  /// **'Inspect file diffs'**
  String get toolDiff;

  /// No description provided for @changedFiles.
  ///
  /// In en, this message translates to:
  /// **'3 files changed'**
  String get changedFiles;

  /// No description provided for @approvalBody.
  ///
  /// In en, this message translates to:
  /// **'Run tests in the sailry-web worktree on Studio'**
  String get approvalBody;

  /// No description provided for @approvalResolved.
  ///
  /// In en, this message translates to:
  /// **'Approval resolved'**
  String get approvalResolved;

  /// No description provided for @chatContinue.
  ///
  /// In en, this message translates to:
  /// **'Continue describing your task'**
  String get chatContinue;

  /// No description provided for @describeTask.
  ///
  /// In en, this message translates to:
  /// **'Describe your task'**
  String get describeTask;

  /// No description provided for @send.
  ///
  /// In en, this message translates to:
  /// **'Send'**
  String get send;

  /// No description provided for @attach.
  ///
  /// In en, this message translates to:
  /// **'Attach'**
  String get attach;

  /// No description provided for @voice.
  ///
  /// In en, this message translates to:
  /// **'Voice input'**
  String get voice;

  /// No description provided for @voiceNote.
  ///
  /// In en, this message translates to:
  /// **'This preview does not access the microphone'**
  String get voiceNote;

  /// No description provided for @attachmentNote.
  ///
  /// In en, this message translates to:
  /// **'Sample attachment added'**
  String get attachmentNote;

  /// No description provided for @attachment.
  ///
  /// In en, this message translates to:
  /// **'design-notes.md'**
  String get attachment;

  /// No description provided for @removeAttachment.
  ///
  /// In en, this message translates to:
  /// **'Remove attachment'**
  String get removeAttachment;

  /// No description provided for @sentPreview.
  ///
  /// In en, this message translates to:
  /// **'Preview only, not sent'**
  String get sentPreview;

  /// No description provided for @model.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get model;

  /// No description provided for @modelSource.
  ///
  /// In en, this message translates to:
  /// **'Current chat configuration · Studio'**
  String get modelSource;

  /// No description provided for @copy.
  ///
  /// In en, this message translates to:
  /// **'Copy'**
  String get copy;

  /// No description provided for @copied.
  ///
  /// In en, this message translates to:
  /// **'Copied'**
  String get copied;

  /// No description provided for @copyFailed.
  ///
  /// In en, this message translates to:
  /// **'Copy unavailable; select the text manually'**
  String get copyFailed;

  /// No description provided for @more.
  ///
  /// In en, this message translates to:
  /// **'More'**
  String get more;

  /// No description provided for @close.
  ///
  /// In en, this message translates to:
  /// **'Close'**
  String get close;

  /// No description provided for @back.
  ///
  /// In en, this message translates to:
  /// **'Back'**
  String get back;

  /// No description provided for @cancel.
  ///
  /// In en, this message translates to:
  /// **'Cancel'**
  String get cancel;

  /// No description provided for @save.
  ///
  /// In en, this message translates to:
  /// **'Save'**
  String get save;

  /// No description provided for @select.
  ///
  /// In en, this message translates to:
  /// **'Select'**
  String get select;

  /// No description provided for @sessionActions.
  ///
  /// In en, this message translates to:
  /// **'Chat actions'**
  String get sessionActions;

  /// No description provided for @queue.
  ///
  /// In en, this message translates to:
  /// **'Message queue'**
  String get queue;

  /// No description provided for @queueEmpty.
  ///
  /// In en, this message translates to:
  /// **'No queued messages'**
  String get queueEmpty;

  /// No description provided for @fork.
  ///
  /// In en, this message translates to:
  /// **'Fork chat'**
  String get fork;

  /// No description provided for @forked.
  ///
  /// In en, this message translates to:
  /// **'Sample fork created'**
  String get forked;

  /// No description provided for @archive.
  ///
  /// In en, this message translates to:
  /// **'Archive chat'**
  String get archive;

  /// No description provided for @archived.
  ///
  /// In en, this message translates to:
  /// **'Archived in preview'**
  String get archived;

  /// No description provided for @stop.
  ///
  /// In en, this message translates to:
  /// **'Stop task'**
  String get stop;

  /// No description provided for @stopped.
  ///
  /// In en, this message translates to:
  /// **'Task stopped in preview'**
  String get stopped;

  /// No description provided for @stoppedStatus.
  ///
  /// In en, this message translates to:
  /// **'Stopped'**
  String get stoppedStatus;

  /// No description provided for @hostSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Your execution Nodes'**
  String get hostSubtitle;

  /// No description provided for @pair.
  ///
  /// In en, this message translates to:
  /// **'Connect host'**
  String get pair;

  /// No description provided for @online.
  ///
  /// In en, this message translates to:
  /// **'Online'**
  String get online;

  /// No description provided for @offline.
  ///
  /// In en, this message translates to:
  /// **'Offline'**
  String get offline;

  /// No description provided for @connection.
  ///
  /// In en, this message translates to:
  /// **'Connection'**
  String get connection;

  /// No description provided for @studio.
  ///
  /// In en, this message translates to:
  /// **'Studio'**
  String get studio;

  /// No description provided for @server.
  ///
  /// In en, this message translates to:
  /// **'Build Server'**
  String get server;

  /// No description provided for @laptop.
  ///
  /// In en, this message translates to:
  /// **'MacBook Air'**
  String get laptop;

  /// No description provided for @hostSystem.
  ///
  /// In en, this message translates to:
  /// **'macOS · Apple Silicon'**
  String get hostSystem;

  /// No description provided for @serverSystem.
  ///
  /// In en, this message translates to:
  /// **'Linux · 8 cores'**
  String get serverSystem;

  /// No description provided for @laptopSystem.
  ///
  /// In en, this message translates to:
  /// **'Last online 2 hours ago'**
  String get laptopSystem;

  /// No description provided for @statusHealthy.
  ///
  /// In en, this message translates to:
  /// **'Healthy'**
  String get statusHealthy;

  /// No description provided for @cpu.
  ///
  /// In en, this message translates to:
  /// **'CPU'**
  String get cpu;

  /// No description provided for @memory.
  ///
  /// In en, this message translates to:
  /// **'Memory'**
  String get memory;

  /// No description provided for @disk.
  ///
  /// In en, this message translates to:
  /// **'Disk'**
  String get disk;

  /// No description provided for @metrics.
  ///
  /// In en, this message translates to:
  /// **'Resource usage'**
  String get metrics;

  /// No description provided for @activity.
  ///
  /// In en, this message translates to:
  /// **'Activity'**
  String get activity;

  /// No description provided for @lastHour.
  ///
  /// In en, this message translates to:
  /// **'Last 60 minutes'**
  String get lastHour;

  /// No description provided for @sessionCount.
  ///
  /// In en, this message translates to:
  /// **'Chats'**
  String get sessionCount;

  /// No description provided for @terminalCount.
  ///
  /// In en, this message translates to:
  /// **'Terminals'**
  String get terminalCount;

  /// No description provided for @projectCount.
  ///
  /// In en, this message translates to:
  /// **'Projects'**
  String get projectCount;

  /// No description provided for @processes.
  ///
  /// In en, this message translates to:
  /// **'Processes'**
  String get processes;

  /// No description provided for @process.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get process;

  /// No description provided for @network.
  ///
  /// In en, this message translates to:
  /// **'Network'**
  String get network;

  /// No description provided for @details.
  ///
  /// In en, this message translates to:
  /// **'Details'**
  String get details;

  /// No description provided for @manageHost.
  ///
  /// In en, this message translates to:
  /// **'Host details'**
  String get manageHost;

  /// No description provided for @hostProjects.
  ///
  /// In en, this message translates to:
  /// **'Host projects'**
  String get hostProjects;

  /// No description provided for @connectionDetails.
  ///
  /// In en, this message translates to:
  /// **'Connection details'**
  String get connectionDetails;

  /// No description provided for @direct.
  ///
  /// In en, this message translates to:
  /// **'Direct'**
  String get direct;

  /// No description provided for @relay.
  ///
  /// In en, this message translates to:
  /// **'Relay'**
  String get relay;

  /// No description provided for @latency.
  ///
  /// In en, this message translates to:
  /// **'Latency'**
  String get latency;

  /// No description provided for @hostOffline.
  ///
  /// In en, this message translates to:
  /// **'Host offline; showing its last known state'**
  String get hostOffline;

  /// No description provided for @retry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get retry;

  /// No description provided for @retryNote.
  ///
  /// In en, this message translates to:
  /// **'Preview is not connected to a real host'**
  String get retryNote;

  /// No description provided for @pairTitle.
  ///
  /// In en, this message translates to:
  /// **'Connect a host'**
  String get pairTitle;

  /// No description provided for @pairDescription.
  ///
  /// In en, this message translates to:
  /// **'Enter the 6-digit pairing code shown on the host'**
  String get pairDescription;

  /// No description provided for @pairCode.
  ///
  /// In en, this message translates to:
  /// **'Pairing code'**
  String get pairCode;

  /// No description provided for @pairHint.
  ///
  /// In en, this message translates to:
  /// **'Pairing code expires in 60 seconds'**
  String get pairHint;

  /// No description provided for @pairDemo.
  ///
  /// In en, this message translates to:
  /// **'Simulate connection'**
  String get pairDemo;

  /// No description provided for @pairSuccess.
  ///
  /// In en, this message translates to:
  /// **'Sample host added'**
  String get pairSuccess;

  /// No description provided for @pairInvalid.
  ///
  /// In en, this message translates to:
  /// **'Enter 6 digits'**
  String get pairInvalid;

  /// No description provided for @workspace.
  ///
  /// In en, this message translates to:
  /// **'Workspace'**
  String get workspace;

  /// No description provided for @workspaceSub.
  ///
  /// In en, this message translates to:
  /// **'Studio / sailry-web'**
  String get workspaceSub;

  /// No description provided for @selectHost.
  ///
  /// In en, this message translates to:
  /// **'Choose host'**
  String get selectHost;

  /// No description provided for @selectProject.
  ///
  /// In en, this message translates to:
  /// **'Choose project'**
  String get selectProject;

  /// No description provided for @selectBranch.
  ///
  /// In en, this message translates to:
  /// **'Choose worktree'**
  String get selectBranch;

  /// No description provided for @mainBranch.
  ///
  /// In en, this message translates to:
  /// **'Main worktree'**
  String get mainBranch;

  /// No description provided for @featureBranch.
  ///
  /// In en, this message translates to:
  /// **'Login layout'**
  String get featureBranch;

  /// No description provided for @connectionTools.
  ///
  /// In en, this message translates to:
  /// **'Connections'**
  String get connectionTools;

  /// No description provided for @workspaceResources.
  ///
  /// In en, this message translates to:
  /// **'Workspace'**
  String get workspaceResources;

  /// No description provided for @confirm.
  ///
  /// In en, this message translates to:
  /// **'Confirm'**
  String get confirm;

  /// No description provided for @git.
  ///
  /// In en, this message translates to:
  /// **'Git'**
  String get git;

  /// No description provided for @gitSummary.
  ///
  /// In en, this message translates to:
  /// **'Changes, branches and history'**
  String get gitSummary;

  /// No description provided for @gitBranches.
  ///
  /// In en, this message translates to:
  /// **'Branches'**
  String get gitBranches;

  /// No description provided for @gitHistory.
  ///
  /// In en, this message translates to:
  /// **'History'**
  String get gitHistory;

  /// No description provided for @gitLineChanges.
  ///
  /// In en, this message translates to:
  /// **'{added} lines added · {removed} removed'**
  String gitLineChanges(String added, String removed);

  /// No description provided for @gitActions.
  ///
  /// In en, this message translates to:
  /// **'Git actions'**
  String get gitActions;

  /// No description provided for @gitFetch.
  ///
  /// In en, this message translates to:
  /// **'Fetch'**
  String get gitFetch;

  /// No description provided for @gitPull.
  ///
  /// In en, this message translates to:
  /// **'Pull'**
  String get gitPull;

  /// No description provided for @gitPush.
  ///
  /// In en, this message translates to:
  /// **'Push'**
  String get gitPush;

  /// No description provided for @gitCurrent.
  ///
  /// In en, this message translates to:
  /// **'Current branch'**
  String get gitCurrent;

  /// No description provided for @gitCreateBranch.
  ///
  /// In en, this message translates to:
  /// **'New branch'**
  String get gitCreateBranch;

  /// No description provided for @gitBranchName.
  ///
  /// In en, this message translates to:
  /// **'Branch name'**
  String get gitBranchName;

  /// No description provided for @gitSwitch.
  ///
  /// In en, this message translates to:
  /// **'Switch branch'**
  String get gitSwitch;

  /// No description provided for @gitMerge.
  ///
  /// In en, this message translates to:
  /// **'Merge branch'**
  String get gitMerge;

  /// No description provided for @gitDeleteBranch.
  ///
  /// In en, this message translates to:
  /// **'Delete branch'**
  String get gitDeleteBranch;

  /// No description provided for @gitHistoryLayout.
  ///
  /// In en, this message translates to:
  /// **'Adjust login form spacing'**
  String get gitHistoryLayout;

  /// No description provided for @gitHistoryInit.
  ///
  /// In en, this message translates to:
  /// **'Initialize login page'**
  String get gitHistoryInit;

  /// No description provided for @gitPreview.
  ///
  /// In en, this message translates to:
  /// **'Git simulation only; repository unchanged'**
  String get gitPreview;

  /// No description provided for @gitDirty.
  ///
  /// In en, this message translates to:
  /// **'Commit current changes first'**
  String get gitDirty;

  /// No description provided for @gitSwitchNote.
  ///
  /// In en, this message translates to:
  /// **'Switch branch in this worktree; simulation only'**
  String get gitSwitchNote;

  /// No description provided for @gitDeleteNote.
  ///
  /// In en, this message translates to:
  /// **'Delete selected branch; simulation only'**
  String get gitDeleteNote;

  /// No description provided for @gitInvalidBranch.
  ///
  /// In en, this message translates to:
  /// **'Invalid name or branch already exists'**
  String get gitInvalidBranch;

  /// No description provided for @review.
  ///
  /// In en, this message translates to:
  /// **'Review'**
  String get review;

  /// No description provided for @browseFiles.
  ///
  /// In en, this message translates to:
  /// **'Browse worktree'**
  String get browseFiles;

  /// No description provided for @reviewFiles.
  ///
  /// In en, this message translates to:
  /// **'View code changes'**
  String get reviewFiles;

  /// No description provided for @selectWorkspace.
  ///
  /// In en, this message translates to:
  /// **'Project and worktree'**
  String get selectWorkspace;

  /// No description provided for @resourceSummary.
  ///
  /// In en, this message translates to:
  /// **'2 chats · 1 terminal'**
  String get resourceSummary;

  /// No description provided for @searchFiles.
  ///
  /// In en, this message translates to:
  /// **'Search files'**
  String get searchFiles;

  /// No description provided for @recentFiles.
  ///
  /// In en, this message translates to:
  /// **'Files'**
  String get recentFiles;

  /// No description provided for @src.
  ///
  /// In en, this message translates to:
  /// **'Source'**
  String get src;

  /// No description provided for @folder.
  ///
  /// In en, this message translates to:
  /// **'Folder'**
  String get folder;

  /// No description provided for @modified.
  ///
  /// In en, this message translates to:
  /// **'Modified'**
  String get modified;

  /// No description provided for @filePreview.
  ///
  /// In en, this message translates to:
  /// **'File preview'**
  String get filePreview;

  /// No description provided for @fileSample.
  ///
  /// In en, this message translates to:
  /// **'Sample file content'**
  String get fileSample;

  /// No description provided for @edit.
  ///
  /// In en, this message translates to:
  /// **'Edit'**
  String get edit;

  /// No description provided for @savePreview.
  ///
  /// In en, this message translates to:
  /// **'Changes saved in this preview'**
  String get savePreview;

  /// No description provided for @unsaved.
  ///
  /// In en, this message translates to:
  /// **'Unsaved'**
  String get unsaved;

  /// No description provided for @discard.
  ///
  /// In en, this message translates to:
  /// **'Discard changes'**
  String get discard;

  /// No description provided for @discardConfirm.
  ///
  /// In en, this message translates to:
  /// **'Discard unsaved changes to this file?'**
  String get discardConfirm;

  /// No description provided for @markdownExample.
  ///
  /// In en, this message translates to:
  /// **'# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev'**
  String get markdownExample;

  /// No description provided for @connections.
  ///
  /// In en, this message translates to:
  /// **'More resources'**
  String get connections;

  /// No description provided for @ssh.
  ///
  /// In en, this message translates to:
  /// **'SSH'**
  String get ssh;

  /// No description provided for @database.
  ///
  /// In en, this message translates to:
  /// **'Database'**
  String get database;

  /// No description provided for @ports.
  ///
  /// In en, this message translates to:
  /// **'Port forwarding'**
  String get ports;

  /// No description provided for @browser.
  ///
  /// In en, this message translates to:
  /// **'Web preview'**
  String get browser;

  /// No description provided for @portsSub.
  ///
  /// In en, this message translates to:
  /// **'1 forward'**
  String get portsSub;

  /// No description provided for @connectionOwner.
  ///
  /// In en, this message translates to:
  /// **'Execution Node · Studio'**
  String get connectionOwner;

  /// No description provided for @openTerminal.
  ///
  /// In en, this message translates to:
  /// **'Open terminal'**
  String get openTerminal;

  /// No description provided for @tables.
  ///
  /// In en, this message translates to:
  /// **'Tables'**
  String get tables;

  /// No description provided for @portNote.
  ///
  /// In en, this message translates to:
  /// **'Sample forward · No local port listener'**
  String get portNote;

  /// No description provided for @portTarget.
  ///
  /// In en, this message translates to:
  /// **'Target port'**
  String get portTarget;

  /// No description provided for @localPort.
  ///
  /// In en, this message translates to:
  /// **'Local port'**
  String get localPort;

  /// No description provided for @closePort.
  ///
  /// In en, this message translates to:
  /// **'Close forward'**
  String get closePort;

  /// No description provided for @portClosed.
  ///
  /// In en, this message translates to:
  /// **'Sample forward closed'**
  String get portClosed;

  /// No description provided for @diffSubtitle.
  ///
  /// In en, this message translates to:
  /// **'sailry-web · feature/sign-in'**
  String get diffSubtitle;

  /// No description provided for @workingTree.
  ///
  /// In en, this message translates to:
  /// **'Working tree'**
  String get workingTree;

  /// No description provided for @staged.
  ///
  /// In en, this message translates to:
  /// **'Staged'**
  String get staged;

  /// No description provided for @diffSummary.
  ///
  /// In en, this message translates to:
  /// **'3 files'**
  String get diffSummary;

  /// No description provided for @stage.
  ///
  /// In en, this message translates to:
  /// **'Stage all'**
  String get stage;

  /// No description provided for @unstage.
  ///
  /// In en, this message translates to:
  /// **'Unstage'**
  String get unstage;

  /// No description provided for @commit.
  ///
  /// In en, this message translates to:
  /// **'Commit'**
  String get commit;

  /// No description provided for @commitTitle.
  ///
  /// In en, this message translates to:
  /// **'Commit changes'**
  String get commitTitle;

  /// No description provided for @commitMessage.
  ///
  /// In en, this message translates to:
  /// **'Commit message'**
  String get commitMessage;

  /// No description provided for @commitPlaceholder.
  ///
  /// In en, this message translates to:
  /// **'Describe the changes'**
  String get commitPlaceholder;

  /// No description provided for @commitPreview.
  ///
  /// In en, this message translates to:
  /// **'Simulate commit'**
  String get commitPreview;

  /// No description provided for @committed.
  ///
  /// In en, this message translates to:
  /// **'Sample commit completed'**
  String get committed;

  /// No description provided for @noChanges.
  ///
  /// In en, this message translates to:
  /// **'No changes to commit'**
  String get noChanges;

  /// No description provided for @diffFile.
  ///
  /// In en, this message translates to:
  /// **'login.css'**
  String get diffFile;

  /// No description provided for @diffPath.
  ///
  /// In en, this message translates to:
  /// **'src/styles/login.css'**
  String get diffPath;

  /// No description provided for @continueEdit.
  ///
  /// In en, this message translates to:
  /// **'Continue editing'**
  String get continueEdit;

  /// No description provided for @diffSelection.
  ///
  /// In en, this message translates to:
  /// **'Choose changed file'**
  String get diffSelection;

  /// No description provided for @terminalKeyboard.
  ///
  /// In en, this message translates to:
  /// **'Keyboard'**
  String get terminalKeyboard;

  /// No description provided for @terminalEnter.
  ///
  /// In en, this message translates to:
  /// **'Enter'**
  String get terminalEnter;

  /// No description provided for @terminalOutputLabel.
  ///
  /// In en, this message translates to:
  /// **'Terminal output'**
  String get terminalOutputLabel;

  /// No description provided for @terminalSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Studio / sailry-web'**
  String get terminalSubtitle;

  /// No description provided for @readOnly.
  ///
  /// In en, this message translates to:
  /// **'Read only'**
  String get readOnly;

  /// No description provided for @takeControl.
  ///
  /// In en, this message translates to:
  /// **'Take control'**
  String get takeControl;

  /// No description provided for @hasControl.
  ///
  /// In en, this message translates to:
  /// **'Input control'**
  String get hasControl;

  /// No description provided for @releaseControl.
  ///
  /// In en, this message translates to:
  /// **'Release control'**
  String get releaseControl;

  /// No description provided for @terminalPlaceholder.
  ///
  /// In en, this message translates to:
  /// **'Enter a sample command'**
  String get terminalPlaceholder;

  /// No description provided for @terminalPreview.
  ///
  /// In en, this message translates to:
  /// **'Sample terminal · Commands are not executed'**
  String get terminalPreview;

  /// No description provided for @terminalOutput.
  ///
  /// In en, this message translates to:
  /// **'Command received in preview, not executed'**
  String get terminalOutput;

  /// No description provided for @terminalControlNote.
  ///
  /// In en, this message translates to:
  /// **'Take control to send input; simulated here'**
  String get terminalControlNote;

  /// No description provided for @usageSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Sailry chats only'**
  String get usageSubtitle;

  /// No description provided for @week.
  ///
  /// In en, this message translates to:
  /// **'This week'**
  String get week;

  /// No description provided for @month.
  ///
  /// In en, this message translates to:
  /// **'This month'**
  String get month;

  /// No description provided for @tokens.
  ///
  /// In en, this message translates to:
  /// **'Tokens'**
  String get tokens;

  /// No description provided for @requests.
  ///
  /// In en, this message translates to:
  /// **'Responses'**
  String get requests;

  /// No description provided for @usageCoverage.
  ///
  /// In en, this message translates to:
  /// **'{priced} / {total} responses priced'**
  String usageCoverage(String priced, String total);

  /// No description provided for @usageEmpty.
  ///
  /// In en, this message translates to:
  /// **'No usage data'**
  String get usageEmpty;

  /// No description provided for @estimatedCost.
  ///
  /// In en, this message translates to:
  /// **'Estimated cost'**
  String get estimatedCost;

  /// No description provided for @costCoverage.
  ///
  /// In en, this message translates to:
  /// **'42 / 48 responses priced'**
  String get costCoverage;

  /// No description provided for @partial.
  ///
  /// In en, this message translates to:
  /// **'Partial data'**
  String get partial;

  /// No description provided for @sourcesPartial.
  ///
  /// In en, this message translates to:
  /// **'2 / 3 hosts updated'**
  String get sourcesPartial;

  /// No description provided for @input.
  ///
  /// In en, this message translates to:
  /// **'Input'**
  String get input;

  /// No description provided for @output.
  ///
  /// In en, this message translates to:
  /// **'Output'**
  String get output;

  /// No description provided for @cached.
  ///
  /// In en, this message translates to:
  /// **'Cache hits'**
  String get cached;

  /// No description provided for @modelUsage.
  ///
  /// In en, this message translates to:
  /// **'Model distribution'**
  String get modelUsage;

  /// No description provided for @hostUsage.
  ///
  /// In en, this message translates to:
  /// **'Host usage'**
  String get hostUsage;

  /// No description provided for @recentRequests.
  ///
  /// In en, this message translates to:
  /// **'Recent responses'**
  String get recentRequests;

  /// No description provided for @allUsage.
  ///
  /// In en, this message translates to:
  /// **'Usage details'**
  String get allUsage;

  /// No description provided for @usageNote.
  ///
  /// In en, this message translates to:
  /// **'Costs are estimates; some responses are not priced'**
  String get usageNote;

  /// No description provided for @sourceNote.
  ///
  /// In en, this message translates to:
  /// **'Offline hosts retain their last known data'**
  String get sourceNote;

  /// No description provided for @profileSubtitle.
  ///
  /// In en, this message translates to:
  /// **'Mobile controller'**
  String get profileSubtitle;

  /// No description provided for @localSettings.
  ///
  /// In en, this message translates to:
  /// **'Local preferences'**
  String get localSettings;

  /// No description provided for @nodeSettings.
  ///
  /// In en, this message translates to:
  /// **'Execution Node settings'**
  String get nodeSettings;

  /// No description provided for @appearance.
  ///
  /// In en, this message translates to:
  /// **'Appearance'**
  String get appearance;

  /// No description provided for @notifications.
  ///
  /// In en, this message translates to:
  /// **'Notifications'**
  String get notifications;

  /// No description provided for @enabled.
  ///
  /// In en, this message translates to:
  /// **'On'**
  String get enabled;

  /// No description provided for @disabled.
  ///
  /// In en, this message translates to:
  /// **'Off'**
  String get disabled;

  /// No description provided for @add.
  ///
  /// In en, this message translates to:
  /// **'Add'**
  String get add;

  /// No description provided for @configName.
  ///
  /// In en, this message translates to:
  /// **'Name'**
  String get configName;

  /// No description provided for @configEndpoint.
  ///
  /// In en, this message translates to:
  /// **'Endpoint'**
  String get configEndpoint;

  /// No description provided for @configModels.
  ///
  /// In en, this message translates to:
  /// **'Models'**
  String get configModels;

  /// No description provided for @configInstructions.
  ///
  /// In en, this message translates to:
  /// **'Instructions'**
  String get configInstructions;

  /// No description provided for @configContent.
  ///
  /// In en, this message translates to:
  /// **'Content'**
  String get configContent;

  /// No description provided for @configEmpty.
  ///
  /// In en, this message translates to:
  /// **'No entries'**
  String get configEmpty;

  /// No description provided for @configDuplicate.
  ///
  /// In en, this message translates to:
  /// **'Name already exists'**
  String get configDuplicate;

  /// No description provided for @configDelete.
  ///
  /// In en, this message translates to:
  /// **'Delete “{name}”?'**
  String configDelete(String name);

  /// No description provided for @speechInput.
  ///
  /// In en, this message translates to:
  /// **'Voice input'**
  String get speechInput;

  /// No description provided for @developerInstructions.
  ///
  /// In en, this message translates to:
  /// **'Change the code for the task and verify the result'**
  String get developerInstructions;

  /// No description provided for @reviewerInstructions.
  ///
  /// In en, this message translates to:
  /// **'Review code changes and identify issues'**
  String get reviewerInstructions;

  /// No description provided for @projectConventions.
  ///
  /// In en, this message translates to:
  /// **'Project conventions'**
  String get projectConventions;

  /// No description provided for @memoryContent.
  ///
  /// In en, this message translates to:
  /// **'Preserve the existing code style'**
  String get memoryContent;

  /// No description provided for @providers.
  ///
  /// In en, this message translates to:
  /// **'Models and providers'**
  String get providers;

  /// No description provided for @roles.
  ///
  /// In en, this message translates to:
  /// **'Roles'**
  String get roles;

  /// No description provided for @memorySettings.
  ///
  /// In en, this message translates to:
  /// **'Memory'**
  String get memorySettings;

  /// No description provided for @speech.
  ///
  /// In en, this message translates to:
  /// **'Speech'**
  String get speech;

  /// No description provided for @nodeSettingsNote.
  ///
  /// In en, this message translates to:
  /// **'Configuration saved on {host}'**
  String nodeSettingsNote(String host);

  /// No description provided for @about.
  ///
  /// In en, this message translates to:
  /// **'About Sailry'**
  String get about;

  /// No description provided for @aboutBody.
  ///
  /// In en, this message translates to:
  /// **'Mobile interaction preview, not connected to services'**
  String get aboutBody;

  /// No description provided for @settingsSaved.
  ///
  /// In en, this message translates to:
  /// **'Settings updated in this preview'**
  String get settingsSaved;

  /// No description provided for @modelPicker.
  ///
  /// In en, this message translates to:
  /// **'Choose model'**
  String get modelPicker;

  /// No description provided for @nodeDefaults.
  ///
  /// In en, this message translates to:
  /// **'Node defaults'**
  String get nodeDefaults;

  /// No description provided for @providerNote.
  ///
  /// In en, this message translates to:
  /// **'Sample configuration · Credentials stay on the execution Node'**
  String get providerNote;

  /// No description provided for @roleNote.
  ///
  /// In en, this message translates to:
  /// **'Sample role · Applies to new chats'**
  String get roleNote;

  /// No description provided for @auto.
  ///
  /// In en, this message translates to:
  /// **'Auto'**
  String get auto;

  /// No description provided for @manual.
  ///
  /// In en, this message translates to:
  /// **'Ask each time'**
  String get manual;

  /// No description provided for @notificationsNote.
  ///
  /// In en, this message translates to:
  /// **'Controls preview notifications only'**
  String get notificationsNote;

  /// No description provided for @memoryNote.
  ///
  /// In en, this message translates to:
  /// **'Sample Node memory'**
  String get memoryNote;

  /// No description provided for @speechNote.
  ///
  /// In en, this message translates to:
  /// **'Uses the execution Node speech configuration'**
  String get speechNote;

  /// No description provided for @newTaskHost.
  ///
  /// In en, this message translates to:
  /// **'Execution host'**
  String get newTaskHost;

  /// No description provided for @newTaskProject.
  ///
  /// In en, this message translates to:
  /// **'Project'**
  String get newTaskProject;

  /// No description provided for @newTaskWorktree.
  ///
  /// In en, this message translates to:
  /// **'Worktree'**
  String get newTaskWorktree;

  /// No description provided for @create.
  ///
  /// In en, this message translates to:
  /// **'Create'**
  String get create;

  /// No description provided for @taskCreated.
  ///
  /// In en, this message translates to:
  /// **'Sample chat created'**
  String get taskCreated;

  /// No description provided for @required.
  ///
  /// In en, this message translates to:
  /// **'Describe your task first'**
  String get required;

  /// No description provided for @notificationsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No new notifications'**
  String get notificationsEmpty;

  /// No description provided for @reviewTitle.
  ///
  /// In en, this message translates to:
  /// **'Design references'**
  String get reviewTitle;

  /// No description provided for @reviewIntro.
  ///
  /// In en, this message translates to:
  /// **'Pages follow the current source; this preview does not establish mobile service acceptance'**
  String get reviewIntro;

  /// No description provided for @reviewConversation.
  ///
  /// In en, this message translates to:
  /// **'Chats, approvals, questions and queue'**
  String get reviewConversation;

  /// No description provided for @reviewConversationText.
  ///
  /// In en, this message translates to:
  /// **'Tasks retain host, project and worktree ownership; expand tool records and approvals within chats'**
  String get reviewConversationText;

  /// No description provided for @reviewResources.
  ///
  /// In en, this message translates to:
  /// **'Files, Git, terminals and connections'**
  String get reviewResources;

  /// No description provided for @reviewResourcesText.
  ///
  /// In en, this message translates to:
  /// **'File editing, staging, commits and ports retain their entry points; details open on secondary pages'**
  String get reviewResourcesText;

  /// No description provided for @reviewHosts.
  ///
  /// In en, this message translates to:
  /// **'Host connections and monitoring'**
  String get reviewHosts;

  /// No description provided for @reviewHostsText.
  ///
  /// In en, this message translates to:
  /// **'Paired Nodes, 6-digit codes, resource usage and processes; offline state is not shown as live'**
  String get reviewHostsText;

  /// No description provided for @reviewUsage.
  ///
  /// In en, this message translates to:
  /// **'Usage and Node settings'**
  String get reviewUsage;

  /// No description provided for @reviewUsageText.
  ///
  /// In en, this message translates to:
  /// **'Sailry chats only; aggregated usage retains completeness and costs indicate estimates and coverage'**
  String get reviewUsageText;

  /// No description provided for @reviewBoundary.
  ///
  /// In en, this message translates to:
  /// **'Mobile boundary'**
  String get reviewBoundary;

  /// No description provided for @reviewBoundaryText.
  ///
  /// In en, this message translates to:
  /// **'The mobile bridge exposes connections, chats, terminals and usage; this preview starts no Node, models, pairing, terminals or plugins'**
  String get reviewBoundaryText;

  /// No description provided for @reviewVisual.
  ///
  /// In en, this message translates to:
  /// **'Visual references'**
  String get reviewVisual;

  /// No description provided for @reviewVisualText.
  ///
  /// In en, this message translates to:
  /// **'Reference 1: chat hierarchy; reference 2: soft cards and floating navigation; reference 3: compact monitoring'**
  String get reviewVisualText;

  /// No description provided for @hostConnectPrompt.
  ///
  /// In en, this message translates to:
  /// **'Connect a host'**
  String get hostConnectPrompt;

  /// No description provided for @hostDisconnected.
  ///
  /// In en, this message translates to:
  /// **'Connection lost'**
  String get hostDisconnected;

  /// No description provided for @language.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get language;

  /// No description provided for @languageSystem.
  ///
  /// In en, this message translates to:
  /// **'System'**
  String get languageSystem;

  /// No description provided for @languageChinese.
  ///
  /// In en, this message translates to:
  /// **'中文'**
  String get languageChinese;

  /// No description provided for @languageEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get languageEnglish;

  /// No description provided for @backgroundConnection.
  ///
  /// In en, this message translates to:
  /// **'Keep connected in background'**
  String get backgroundConnection;

  /// No description provided for @backgroundConnectionActive.
  ///
  /// In en, this message translates to:
  /// **'Keeping host connections active'**
  String get backgroundConnectionActive;

  /// No description provided for @backgroundConnectionFailed.
  ///
  /// In en, this message translates to:
  /// **'Background connection not enabled; retry'**
  String get backgroundConnectionFailed;

  /// No description provided for @resetReasoning.
  ///
  /// In en, this message translates to:
  /// **'Reset effort'**
  String get resetReasoning;

  /// No description provided for @completionAlerts.
  ///
  /// In en, this message translates to:
  /// **'Completion alerts'**
  String get completionAlerts;

  /// No description provided for @notificationsReadAll.
  ///
  /// In en, this message translates to:
  /// **'Mark all read'**
  String get notificationsReadAll;

  /// No description provided for @notificationsOpen.
  ///
  /// In en, this message translates to:
  /// **'Open'**
  String get notificationsOpen;

  /// No description provided for @preferencesFailed.
  ///
  /// In en, this message translates to:
  /// **'Preferences not saved; retry'**
  String get preferencesFailed;

  /// No description provided for @connectFirst.
  ///
  /// In en, this message translates to:
  /// **'Connect a host to begin'**
  String get connectFirst;

  /// No description provided for @initializing.
  ///
  /// In en, this message translates to:
  /// **'Starting'**
  String get initializing;

  /// No description provided for @startupFailed.
  ///
  /// In en, this message translates to:
  /// **'Startup failed'**
  String get startupFailed;

  /// No description provided for @retryConnection.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get retryConnection;

  /// No description provided for @pairAction.
  ///
  /// In en, this message translates to:
  /// **'Connect'**
  String get pairAction;

  /// No description provided for @pairFailed.
  ///
  /// In en, this message translates to:
  /// **'Connection failed; retry'**
  String get pairFailed;

  /// No description provided for @pairExpired.
  ///
  /// In en, this message translates to:
  /// **'Pairing code expired; get a new code'**
  String get pairExpired;

  /// No description provided for @pairing.
  ///
  /// In en, this message translates to:
  /// **'Connecting'**
  String get pairing;

  /// No description provided for @hostUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Host not connected'**
  String get hostUnavailable;

  /// No description provided for @hostMetricsFailed.
  ///
  /// In en, this message translates to:
  /// **'Cannot read host status'**
  String get hostMetricsFailed;

  /// No description provided for @hostProcessesEmpty.
  ///
  /// In en, this message translates to:
  /// **'No processes'**
  String get hostProcessesEmpty;

  /// No description provided for @hostRegisterProject.
  ///
  /// In en, this message translates to:
  /// **'Add project'**
  String get hostRegisterProject;

  /// No description provided for @hostChooseDirectory.
  ///
  /// In en, this message translates to:
  /// **'Choose directory'**
  String get hostChooseDirectory;

  /// No description provided for @hostChooseFile.
  ///
  /// In en, this message translates to:
  /// **'Choose file'**
  String get hostChooseFile;

  /// No description provided for @hostParentDirectory.
  ///
  /// In en, this message translates to:
  /// **'Parent directory'**
  String get hostParentDirectory;

  /// No description provided for @hostEmptyDirectory.
  ///
  /// In en, this message translates to:
  /// **'Directory is empty'**
  String get hostEmptyDirectory;

  /// No description provided for @hostLoadMore.
  ///
  /// In en, this message translates to:
  /// **'Load more'**
  String get hostLoadMore;

  /// No description provided for @hostProjectName.
  ///
  /// In en, this message translates to:
  /// **'Project name'**
  String get hostProjectName;

  /// No description provided for @hostProjectPath.
  ///
  /// In en, this message translates to:
  /// **'Project path on host'**
  String get hostProjectPath;

  /// No description provided for @hostProjectFailed.
  ///
  /// In en, this message translates to:
  /// **'Cannot add project'**
  String get hostProjectFailed;

  /// No description provided for @hostUnknown.
  ///
  /// In en, this message translates to:
  /// **'No data'**
  String get hostUnknown;

  /// No description provided for @hostRefresh.
  ///
  /// In en, this message translates to:
  /// **'Refresh'**
  String get hostRefresh;

  /// No description provided for @hostMetricCpu.
  ///
  /// In en, this message translates to:
  /// **'CPU'**
  String get hostMetricCpu;

  /// No description provided for @hostMetricMemory.
  ///
  /// In en, this message translates to:
  /// **'Memory'**
  String get hostMetricMemory;

  /// No description provided for @hostMetricDisk.
  ///
  /// In en, this message translates to:
  /// **'Disk'**
  String get hostMetricDisk;

  /// No description provided for @failureConflict.
  ///
  /// In en, this message translates to:
  /// **'Content changed; reload and retry'**
  String get failureConflict;

  /// No description provided for @failureUnknown.
  ///
  /// In en, this message translates to:
  /// **'Outcome unconfirmed; check the host state first'**
  String get failureUnknown;

  /// No description provided for @failureDenied.
  ///
  /// In en, this message translates to:
  /// **'Permission denied'**
  String get failureDenied;

  /// No description provided for @failureUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Host not connected'**
  String get failureUnavailable;

  /// No description provided for @failureBusy.
  ///
  /// In en, this message translates to:
  /// **'Service busy; retry later'**
  String get failureBusy;

  /// No description provided for @failureGeneric.
  ///
  /// In en, this message translates to:
  /// **'Operation failed'**
  String get failureGeneric;

  /// No description provided for @settingsSpeechLanguage.
  ///
  /// In en, this message translates to:
  /// **'Language'**
  String get settingsSpeechLanguage;

  /// No description provided for @settingsSpeechAuto.
  ///
  /// In en, this message translates to:
  /// **'Auto-detect'**
  String get settingsSpeechAuto;

  /// No description provided for @settingsSpeechChinese.
  ///
  /// In en, this message translates to:
  /// **'Chinese'**
  String get settingsSpeechChinese;

  /// No description provided for @settingsSpeechEnglish.
  ///
  /// In en, this message translates to:
  /// **'English'**
  String get settingsSpeechEnglish;

  /// No description provided for @settingsSpeechReady.
  ///
  /// In en, this message translates to:
  /// **'Speech model ready'**
  String get settingsSpeechReady;

  /// No description provided for @settingsSpeechDownload.
  ///
  /// In en, this message translates to:
  /// **'Download speech model'**
  String get settingsSpeechDownload;

  /// No description provided for @settingsSpeechFailed.
  ///
  /// In en, this message translates to:
  /// **'Speech model not ready; retry'**
  String get settingsSpeechFailed;

  /// No description provided for @settingsNoHost.
  ///
  /// In en, this message translates to:
  /// **'Connect a host first'**
  String get settingsNoHost;

  /// No description provided for @settingsUnavailable.
  ///
  /// In en, this message translates to:
  /// **'Unavailable'**
  String get settingsUnavailable;

  /// No description provided for @settingsLoadFailed.
  ///
  /// In en, this message translates to:
  /// **'Could not load'**
  String get settingsLoadFailed;

  /// No description provided for @settingsSaveFailed.
  ///
  /// In en, this message translates to:
  /// **'Save failed; draft kept'**
  String get settingsSaveFailed;

  /// No description provided for @settingsConflict.
  ///
  /// In en, this message translates to:
  /// **'Settings changed; reopen and retry'**
  String get settingsConflict;

  /// No description provided for @settingsUnknown.
  ///
  /// In en, this message translates to:
  /// **'Outcome unconfirmed; refresh to check'**
  String get settingsUnknown;

  /// No description provided for @settingsRetry.
  ///
  /// In en, this message translates to:
  /// **'Retry'**
  String get settingsRetry;

  /// No description provided for @settingsLoading.
  ///
  /// In en, this message translates to:
  /// **'Loading'**
  String get settingsLoading;

  /// No description provided for @settingsRequired.
  ///
  /// In en, this message translates to:
  /// **'Enter a value'**
  String get settingsRequired;

  /// No description provided for @settingsKey.
  ///
  /// In en, this message translates to:
  /// **'ID'**
  String get settingsKey;

  /// No description provided for @settingsDescription.
  ///
  /// In en, this message translates to:
  /// **'Description'**
  String get settingsDescription;

  /// No description provided for @settingsInstructions.
  ///
  /// In en, this message translates to:
  /// **'Instructions'**
  String get settingsInstructions;

  /// No description provided for @settingsModels.
  ///
  /// In en, this message translates to:
  /// **'Model IDs, one per line'**
  String get settingsModels;

  /// No description provided for @settingsApi.
  ///
  /// In en, this message translates to:
  /// **'API format'**
  String get settingsApi;

  /// No description provided for @settingsOpenCodeGo.
  ///
  /// In en, this message translates to:
  /// **'OpenCode Go'**
  String get settingsOpenCodeGo;

  /// No description provided for @settingsOpenCodeZen.
  ///
  /// In en, this message translates to:
  /// **'OpenCode Zen'**
  String get settingsOpenCodeZen;

  /// No description provided for @settingsApiKey.
  ///
  /// In en, this message translates to:
  /// **'API Key'**
  String get settingsApiKey;

  /// No description provided for @settingsEnabled.
  ///
  /// In en, this message translates to:
  /// **'Enabled'**
  String get settingsEnabled;

  /// No description provided for @settingsDriver.
  ///
  /// In en, this message translates to:
  /// **'Service'**
  String get settingsDriver;

  /// No description provided for @settingsModel.
  ///
  /// In en, this message translates to:
  /// **'Model'**
  String get settingsModel;

  /// No description provided for @settingsMemoryAuto.
  ///
  /// In en, this message translates to:
  /// **'Record automatically'**
  String get settingsMemoryAuto;

  /// No description provided for @settingsMemoryBudget.
  ///
  /// In en, this message translates to:
  /// **'Context bytes'**
  String get settingsMemoryBudget;

  /// No description provided for @settingsMemoryReview.
  ///
  /// In en, this message translates to:
  /// **'Review interval (days)'**
  String get settingsMemoryReview;

  /// No description provided for @settingsMemoryRecords.
  ///
  /// In en, this message translates to:
  /// **'Memory entries'**
  String get settingsMemoryRecords;

  /// No description provided for @settingsMemoryKind.
  ///
  /// In en, this message translates to:
  /// **'Type'**
  String get settingsMemoryKind;

  /// No description provided for @settingsMemoryUser.
  ///
  /// In en, this message translates to:
  /// **'User'**
  String get settingsMemoryUser;

  /// No description provided for @settingsMemoryFeedback.
  ///
  /// In en, this message translates to:
  /// **'Feedback'**
  String get settingsMemoryFeedback;

  /// No description provided for @settingsMemoryProject.
  ///
  /// In en, this message translates to:
  /// **'Project'**
  String get settingsMemoryProject;

  /// No description provided for @settingsMemoryReference.
  ///
  /// In en, this message translates to:
  /// **'Reference'**
  String get settingsMemoryReference;

  /// No description provided for @settingsArchived.
  ///
  /// In en, this message translates to:
  /// **'Archived'**
  String get settingsArchived;

  /// No description provided for @settingsEmpty.
  ///
  /// In en, this message translates to:
  /// **'No records'**
  String get settingsEmpty;

  /// No description provided for @settingsUsageUnknown.
  ///
  /// In en, this message translates to:
  /// **'Unknown'**
  String get settingsUsageUnknown;

  /// No description provided for @settingsUsagePartial.
  ///
  /// In en, this message translates to:
  /// **'Some hosts are unavailable'**
  String get settingsUsagePartial;

  /// No description provided for @settingsUsageCache.
  ///
  /// In en, this message translates to:
  /// **'Cached'**
  String get settingsUsageCache;

  /// No description provided for @settingsUsageInput.
  ///
  /// In en, this message translates to:
  /// **'Uncached input'**
  String get settingsUsageInput;

  /// No description provided for @settingsUsageOutput.
  ///
  /// In en, this message translates to:
  /// **'Output'**
  String get settingsUsageOutput;

  /// No description provided for @settingsUsageDaily.
  ///
  /// In en, this message translates to:
  /// **'Daily'**
  String get settingsUsageDaily;

  /// No description provided for @settingsUsageWeekly.
  ///
  /// In en, this message translates to:
  /// **'Weekly'**
  String get settingsUsageWeekly;

  /// No description provided for @settingsUtc.
  ///
  /// In en, this message translates to:
  /// **'UTC'**
  String get settingsUtc;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'zh'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'zh':
      return AppLocalizationsZh();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}
