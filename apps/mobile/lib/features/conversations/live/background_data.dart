bool noticeTurn(Map<String, dynamic> run) => run['notice'] == true;

String backgroundLabel(Object? outcome) => switch (outcome) {
  'completed' => 'backgroundCompleted',
  'failed' => 'backgroundFailed',
  'cancelled' => 'backgroundCancelled',
  'stopped' => 'backgroundStopped',
  'timed_out' => 'backgroundTimedOut',
  'interrupted' => 'backgroundInterrupted',
  _ => 'backgroundUnknown',
};

/// Cancelled and stopped results were chosen; the rest did not finish cleanly.
bool backgroundFailed(Object? outcome) =>
    !['completed', 'cancelled', 'stopped'].contains(outcome);
