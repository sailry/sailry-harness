import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../runtime/json.dart';

typedef OutputLoader =
    Future<Map<String, dynamic>?> Function(String entry, int index);

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

class ToolInput extends StatelessWidget {
  const ToolInput({super.key, required this.input});
  final Map<String, dynamic> input;

  @override
  Widget build(BuildContext context) {
    final fields = <String>{};
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final name in ['summary', 'context', 'content'])
          if (object(input[name]) case final field
              when text(field['text']).isNotEmpty &&
                  fields.add(text(field['text'])))
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: field['code'] == true
                  ? CodeBlock(text(field['text']))
                  : SelectableText(text(field['text'])),
            ),
      ],
    );
  }
}
