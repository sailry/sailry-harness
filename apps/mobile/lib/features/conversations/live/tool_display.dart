import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';

typedef ToolSelectionChanged =
    void Function(Object owner, List<Map<String, dynamic>> calls, bool retry);

bool toolFailed(Map<String, dynamic> call) =>
    object(call['row'])['failed'] == true;

String capturedLabel(BuildContext context, Object? value) {
  final label = object(value);
  final locale = Localizations.localeOf(context);
  final locales = object(label['locales']);
  final traditional =
      locale.scriptCode == 'Hant' ||
      (locale.scriptCode != 'Hans' &&
          const ['TW', 'HK', 'MO'].contains(locale.countryCode));
  final alias = locale.languageCode == 'zh'
      ? (traditional ? 'zh-TW' : 'zh-CN')
      : locale.languageCode;
  return text(
    locales[locale.toLanguageTag()] ??
        locales[alias] ??
        locales[locale.languageCode],
    text(label['label']),
  );
}

/// A package's continuation marker in the app's language; a continuation
/// without a declared marker reads as continued.
String continuationLabel(BuildContext context, Object? marker) {
  final label = capturedLabel(context, marker);
  return label.isEmpty ? context.tr('conversationContinued') : label;
}
