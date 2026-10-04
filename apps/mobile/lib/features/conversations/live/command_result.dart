import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';

Map<String, dynamic> commandOutcome(Map<String, dynamic> result) =>
    result['kind'] == 'command_result'
    ? object(object(result['data'])['outcome'])
    : {};

// Shell exit codes are command-specific; a nonzero code is not a tool fault.
bool commandFailed(Map<String, dynamic> result) =>
    switch (commandOutcome(result)['kind']) {
      'signal' || 'timed_out' => true,
      _ => false,
    };

String? commandStatus(Map<String, dynamic> result) {
  final outcome = commandOutcome(result);
  return switch (outcome['kind']) {
    'exited' => tr('toolExitCode').replaceAll('{code}', '${outcome['data']}'),
    'signal' => tr('toolSignal').replaceAll('{signal}', '${outcome['data']}'),
    'timed_out' => tr('toolTimedOut'),
    'cancelled' => tr('toolCancelled'),
    'unknown' => tr('toolOutcomeUnknown'),
    _ => null,
  };
}
