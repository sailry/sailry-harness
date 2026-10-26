import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';

void main() {
  final attachment = {
    'id': 'attachment',
    'spec': {'name': 'captured.png', 'worktree': 'child-tree'},
  };
  final reference = {'target': 'project', 'label': 'Captured project'};
  test('reads the Client input projection without fallback', () {
    final view = {
      'inputs': {
        'turn': [
          {
            'message': {
              'text': 'Original input 中文 🙂',
              'references': [reference],
            },
            'attachments': [attachment],
            'canonical': false,
          },
          {
            'message': {'text': '', 'references': []},
            'attachments': [attachment],
            'canonical': true,
          },
        ],
      },
    };
    final before = jsonEncode(view);
    final inputs = turnInputs(view, 'turn');
    expect(inputs.first.text, 'Original input 中文 🙂');
    expect(inputs.first.references, [reference]);
    expect(inputs.first.attachments, [attachment]);
    expect(inputs.first.canonical, isFalse);
    expect(inputs.last.text, isEmpty);
    expect(inputs.last.canonical, isTrue);
    expect(turnInputs(view, 'other'), isEmpty);
    expect(jsonEncode(view), before);
  });

  test('history actions preserve canonical text boundaries', () {
    final page = {
      'entries': [
        {
          'turn': 'turn',
          'author': 'user',
          'parts': [
            {'kind': 'attachment', 'data': attachment},
          ],
        },
        {
          'turn': 'turn',
          'author': 'user',
          'parts': [
            {'kind': 'text', 'data': 'First'},
            {'kind': 'reference', 'data': reference},
            {'kind': 'text', 'data': 'Second'},
          ],
        },
      ],
    };
    expect(sentInput(page, 'turn'), {
      'text': 'First\nSecond',
      'attachments': ['attachment'],
      'references': [reference],
    });
  });
}
