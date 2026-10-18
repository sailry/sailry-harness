import '../../../runtime/json.dart';

bool noticeTurn(Map<String, dynamic> run) => run['notice'] == true;

List<Map<String, dynamic>> pendingBackground(Map<String, dynamic> page) =>
    objects(page['background'])
        .where(
          (task) =>
              task['state'] == 'pending' &&
              task['delivery'] == null &&
              task['outcome'] != null,
        )
        .toList();

String backgroundLabel(Object? outcome) => switch (outcome) {
  'completed' => 'backgroundCompleted',
  'failed' => 'backgroundFailed',
  'cancelled' => 'backgroundCancelled',
  'stopped' => 'backgroundStopped',
  'timed_out' => 'backgroundTimedOut',
  'interrupted' => 'backgroundInterrupted',
  _ => 'backgroundUnknown',
};
