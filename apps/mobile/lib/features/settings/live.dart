import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart' show AppSession;
export '../../runtime/json.dart' show object, objects, text, number, newId;
import '../resources/scope_picker.dart';

int integer(Object? value) => value is num ? value.toInt() : 0;

String failure(Object error, {bool saving = false, Translator translate = tr}) {
  final detail = error.toString();
  if (detail.contains('revision_conflict')) {
    return translate('settingsConflict');
  }
  if (detail.contains('outcome_unknown')) return translate('settingsUnknown');
  return translate(saving ? 'settingsSaveFailed' : 'settingsLoadFailed');
}

Future<void> pickSettingsHost(BuildContext context, AppSession session) async {
  final selected = await showHostPicker(
    context,
    session,
    selected: session.selectedHost?.id,
  );
  if (selected != null) session.selectHost(selected);
}

Widget settingsError(String? error) => error == null
    ? const SizedBox.shrink()
    : Padding(
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: Semantics(liveRegion: true, child: Text(error)),
      );
