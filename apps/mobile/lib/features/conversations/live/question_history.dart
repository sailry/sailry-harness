import 'package:flutter/material.dart';
import '../../../content/markdown.dart';
import '../../../runtime/json.dart';
import '../../../l10n/strings.dart';

class QuestionHistory extends StatelessWidget {
  const QuestionHistory({
    super.key,
    required this.question,
    required this.spec,
  });
  final Map<String, dynamic> question, spec;
  @override
  Widget build(BuildContext context) {
    final state = object(question['state']);
    final answer = object(state['data']);
    final input = object(spec['input']);
    final values = <String>[];
    switch (answer['kind']) {
      case 'text':
        values.add(text(answer['data']));
      case 'choices':
        final choices = object(answer['data']);
        final options = input['options'] as List? ?? [];
        for (final index in choices['selected'] as List? ?? []) {
          if (index is int && index >= 0 && index < options.length) {
            values.add('${options[index]}');
          }
        }
        if (choices['other'] is String) values.add(choices['other']);
      case 'form':
        final form = object(answer['data']);
        for (final field in objects(input['fields'])) {
          if (form.containsKey(field['name'])) {
            final value = form[field['name']];
            final options = objects(object(field['input'])['options']);
            String label(Object? value) => text(
              options
                  .where((option) => option['value'] == value)
                  .firstOrNull?['title'],
              '$value',
            );
            final displayed = value is List
                ? value.map(label).join(', ')
                : label(value);
            values.add('${field['title']}: $displayed');
          }
        }
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        MarkdownContent(text(spec['prompt'])),
        Text(
          context.tr(
            state['kind'] == 'answered'
                ? 'toolQuestionAnswered'
                : state['kind'] == 'declined'
                ? 'toolQuestionDeclined'
                : state['kind'] == 'interrupted'
                ? 'conversationInterrupted'
                : 'toolQuestionCancelled',
          ),
        ),
        for (final value in values) SelectableText(value),
      ],
    );
  }
}
