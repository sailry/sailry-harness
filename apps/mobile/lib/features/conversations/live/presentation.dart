import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/theme.dart';

typedef ConversationCommand =
    Future<Map<String, dynamic>> Function(
      String kind,
      Map<String, dynamic> data,
    );

String failureLabel(Object error) => error is CommandFailure
    ? tr(
        error.code == 'outcome_unknown'
            ? 'conversationUnknown'
            : error.code == 'revision_conflict'
            ? 'conversationConflict'
            : 'conversationFailed',
      )
    : error is String
    ? error
    : tr('conversationFailed');

String title(Map<String, dynamic> session) {
  final value = object(session['activity'])['title'] as String? ?? '';
  return value.isEmpty ? tr('conversationNew') : value;
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

String label(Map<String, dynamic> session) {
  final activity = object(session['activity']);
  if (activity['waiting'] == 'approval') return tr('approval');
  if (activity['waiting'] == 'input') return tr('questionPending');
  return runLabel(object(activity['run'])['status'] as String?);
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

String runLabel(String? status) => tr(switch (status) {
  'running' => 'running',
  'stopping' => 'conversationStopping',
  'queued' => 'conversationQueued',
  'completed' => 'completed',
  'cancelled' => 'stoppedStatus',
  'interrupted' => 'conversationInterrupted',
  'failed' => 'conversationFailedStatus',
  _ => 'idle',
});
