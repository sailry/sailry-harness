import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/theme.dart';

typedef ConversationCommand =
    Future<Map<String, dynamic>> Function(
      String kind,
      Map<String, dynamic> data,
    );

String failureLabel(Object error, {Translator translate = tr}) =>
    error is CommandFailure
    ? continuationRefusal(error.details, translate: translate) ??
          translate(
            versionMismatchKey(error) ??
                (error.code == 'outcome_unknown'
                    ? 'conversationUnknown'
                    : error.code == 'revision_conflict'
                    ? 'conversationConflict'
                    : error.code == 'context_overflow'
                    ? 'conversationContextOverflow'
                    : 'conversationFailed'),
          )
    : error is String
    ? error
    : translate('conversationFailed');

/// Why the Node admitted no automatic turn for a package, from a fault's
/// `continuation` details; null for any other fault. A reason this build does
/// not know reads as a plain refusal, and the limit names its count when the
/// Node sent one.
String? continuationRefusal(Object? details, {Translator translate = tr}) {
  final refusal = object(details);
  if (refusal['kind'] != 'continuation') return null;
  final count = refusal['count'];
  final limit = refusal['limit'];
  if (refusal['reason'] == 'limit' && count is int && limit is int) {
    return translate(
      'continuationLimitCount',
    ).replaceAll('{count}', '$count').replaceAll('{limit}', '$limit');
  }
  return translate(switch (refusal['reason']) {
    'plan' => 'continuationPlan',
    'child' => 'continuationChild',
    'archived' => 'continuationArchived',
    'worktree' => 'continuationWorktree',
    'queue_paused' => 'continuationQueuePaused',
    'queued_input' => 'continuationQueuedInput',
    'running' => 'continuationRunning',
    'interaction' => 'continuationInteraction',
    'limit' => 'continuationLimit',
    'stale' => 'continuationStale',
    _ => 'continuationRefused',
  });
}

/// The marker of a turn a package continued, shown in place of its input,
/// which is for the model; null for any other turn. A continuation without a
/// declared marker has an empty one and reads as continued.
Map<String, dynamic>? continuationMarker(Map<String, dynamic> run) =>
    run['continuation'] is Map
    ? object(object(run['continuation'])['marker'])
    : null;

/// A failed run's line: the Node's message, under the failure's own label
/// when its code has one.
String runFailure(Object? error, {Translator translate = tr}) {
  final fault = object(error);
  final message = fault['message'] is String
      ? fault['message'] as String
      : null;
  if (fault['code'] != 'context_overflow') {
    return message ?? translate('conversationFailedStatus');
  }
  final label = translate('conversationContextOverflow');
  return message == null || message.isEmpty ? label : '$label\n$message';
}

String title(Map<String, dynamic> session, {Translator translate = tr}) {
  final value = object(session['activity'])['title'] as String? ?? '';
  return value.isEmpty ? translate('conversationNew') : value;
}

/// Resource assistants and delegated sessions keep their own entry points.
bool ordinarySession(Map<String, dynamic> session) {
  final config = object(session['config']);
  return session['delegation'] == null &&
      config['assistant'] == null &&
      config['resource'] == null;
}

String status(Map<String, dynamic> session) {
  final activity = object(session['activity']);
  if (activity['waiting'] != null) return 'waiting';
  return switch (object(activity['run'])['status']) {
    'queued' => 'waiting',
    'running' || 'stopping' => 'running',
    'completed' => 'completed',
    _ => 'idle',
  };
}

String label(Map<String, dynamic> session, {Translator translate = tr}) {
  final activity = object(session['activity']);
  if (activity['waiting'] == 'approval') return translate('approval');
  if (activity['waiting'] == 'input') return translate('questionPending');
  return runLabel(
    object(activity['run'])['status'] as String?,
    translate: translate,
  );
}

StatusTone tone(Map<String, dynamic> session) {
  final activity = object(session['activity']);
  if (activity['waiting'] != null) return StatusTone.warning;
  return switch (object(activity['run'])['status']) {
    'completed' => StatusTone.success,
    'cancelled' || 'interrupted' || 'failed' => StatusTone.danger,
    'queued' => StatusTone.warning,
    'running' || 'stopping' => StatusTone.running,
    _ => StatusTone.neutral,
  };
}

/// The task list's status dot; completed runs show only until they are read.
StatusTone? listTone(Map<String, dynamic> session) {
  final activity = object(session['activity']);
  if (activity['waiting'] != null) return StatusTone.warning;
  return switch (object(activity['run'])['status']) {
    'running' || 'queued' || 'stopping' => StatusTone.running,
    'failed' || 'interrupted' => StatusTone.danger,
    'completed' when object(activity['attention'])['unread'] == true =>
      StatusTone.success,
    _ => null,
  };
}

String runLabel(String? status, {Translator translate = tr}) =>
    translate(switch (status) {
      'running' => 'running',
      'stopping' => 'conversationStopping',
      'queued' => 'conversationQueued',
      'completed' => 'completed',
      'continued' => 'conversationContinued',
      'cancelled' => 'stoppedStatus',
      'interrupted' => 'conversationInterrupted',
      'failed' => 'conversationFailedStatus',
      _ => 'idle',
    });
