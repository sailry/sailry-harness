import '../../../l10n/strings.dart';

/// Client selects the declared command outcome; UI only localizes its text.
String? commandStatus(
  Map<String, dynamic> outcome, {
  Translator translate = tr,
}) => switch (outcome['kind']) {
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
