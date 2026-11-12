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
