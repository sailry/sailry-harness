import '../../../l10n/strings.dart';

bool noticeTurn(Map<String, dynamic> run) => run['notice'] == true;

String backgroundLabel(Object? outcome) => switch (outcome) {
  'completed' => 'backgroundCompleted',
  'failed' => 'backgroundFailed',
  'cancelled' => 'backgroundCancelled',
  'stopped' => 'backgroundStopped',
  'timed_out' => 'backgroundTimedOut',
  'interrupted' => 'backgroundInterrupted',
  'stalled' => 'backgroundStalled',
  _ => 'backgroundUnknown',
};

/// Cancelled and stopped results were chosen; a stalled command is still
/// running. The rest did not finish cleanly.
bool backgroundFailed(Object? outcome) =>
    !['completed', 'cancelled', 'stopped', 'stalled'].contains(outcome);

/// A still-running command without recent output needs a decision.
bool backgroundWarning(Object? outcome) => outcome == 'stalled';

/// A finished result no turn has taken: the Node holds it for the user's next
/// message instead of starting a notice turn.
bool backgroundHeld(Map<String, dynamic> task) =>
    task['state'] == 'pending' &&
    (task['hold'] != null || task['suppressed'] == true);

/// Why a result is held. A missing reason, `other` and one this build does not
/// know read as held without a cause.
String backgroundHoldLabel(Object? hold) => switch (hold) {
  'limit' => 'backgroundHoldLimit',
  'stopped' => 'backgroundHoldStopped',
  'interrupted' => 'backgroundHoldInterrupted',
  'failed' => 'backgroundHoldFailed',
  _ => 'backgroundHeld',
};

/// A held task's line: how it ended, when the Node said, then why it waits.
/// A held result is never an unknown outcome.
String backgroundHoldText(Map<String, dynamic> task, Translator translate) => [
  if (task['outcome'] != null) translate(backgroundLabel(task['outcome'])),
  translate(backgroundHoldLabel(task['hold'])),
].join(' · ');
