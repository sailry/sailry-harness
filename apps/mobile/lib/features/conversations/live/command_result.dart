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

String? commandStatus(
  Map<String, dynamic> result, {
  Translator translate = tr,
}) {
  final outcome = commandOutcome(result);
  return switch (outcome['kind']) {
    'exited' => translate(
      'toolExitCode',
    ).replaceAll('{code}', '${outcome['data']}'),
    'signal' => translate(
      'toolSignal',
    ).replaceAll('{signal}', '${outcome['data']}'),
    'timed_out' => translate('toolTimedOut'),
    'cancelled' => translate('toolCancelled'),
    'unknown' => translate('toolOutcomeUnknown'),
    _ => null,
  };
}
