import 'package:flutter/material.dart';
import '../l10n/strings.dart';
import '../ui/toast.dart';
import 'session.dart';

String failureText(Object failure) {
  final code = failure is CommandFailure ? failure.code : '';
  return tr(switch (code) {
    'outcome_unknown' => 'failureUnknown',
    'revision_conflict' || 'conflict' => 'failureConflict',
    'permission_denied' => 'failureDenied',
    'unavailable' => 'failureUnavailable',
    'busy' => 'failureBusy',
    _ => 'failureGeneric',
  });
}

void showFailure(BuildContext context, Object failure) {
  showToast(context, failureText(failure));
}
