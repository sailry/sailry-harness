import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../runtime/json.dart';
import 'command_result.dart';

bool resultFailed(Map<String, dynamic> result, Map<String, dynamic> resolved) {
  final error = result['error'];
  final fault = object(error);
  if (fault['code'] == 'cancelled' || fault['code'] == 'outcome_unknown') {
    return false;
  }
  return (error is String ? error.isNotEmpty : fault.isNotEmpty) ||
      result['isError'] == true ||
      object(result['output'])['isError'] == true ||
      objects(resolved['diagnostics']).any((item) => item['error'] == true) ||
      commandFailed(result);
}

Object? contentValue(Object? value, String pointer) {
  if (pointer.isEmpty) return value;
  if (!pointer.startsWith('/')) return null;
  for (final segment in pointer.substring(1).split('/')) {
    final key = segment.replaceAll('~1', '/').replaceAll('~0', '~');
    if (value is Map) {
      value = value[key];
    } else if (value is List) {
      final index = int.tryParse(key);
      if (index == null || index < 0 || index >= value.length) return null;
      value = value[index];
    } else {
      return null;
    }
  }
  return value;
}

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
