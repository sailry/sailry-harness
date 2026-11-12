import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart' show HostConnection;
export '../../runtime/json.dart' show object, objects, text, number, newId;

int integer(Object? value) => value is num ? value.toInt() : 0;
String string(Object? value) => value is String ? value : '';

String failure(Object error, {bool saving = false, Translator translate = tr}) {
  final detail = error.toString();
  if (detail.contains('revision_conflict')) {
    return translate('settingsConflict');
  }
  if (detail.contains('outcome_unknown')) return translate('settingsUnknown');
  return translate(saving ? 'settingsSaveFailed' : 'settingsLoadFailed');
}

Widget settingsError(String? error) => error == null
    ? const SizedBox.shrink()
    : Padding(
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: Semantics(liveRegion: true, child: Text(error)),
      );

/// A settings sheet captures its execution host; switching the shell's host never
/// redirects an in-progress save to another Node.
typedef SettingsHost = HostConnection;
