import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';

void main() {
  final attachment = {
    'id': 'attachment',
    'spec': {'name': 'captured.png', 'worktree': 'child-tree'},
  };
  final reference = {'target': 'project', 'label': 'Captured project'};
  final capture = {
    'message': {
      'text': 'Original input 中文 🙂',
      'attachments': ['attachment'],
      'references': [reference],
    },
    'attachments': [attachment],
  };

  test('admitted content survives claim and reconnect', () {
    final page = {'entries': <Map<String, dynamic>>[]};
    for (final status in ['queued', 'running', 'failed', 'interrupted']) {
      final run = {'turn': 'turn', 'status': status, 'input': capture};
      final before = jsonEncode([page, run]);
      final input = turnInputs(page, run).single;
      expect(input.text, 'Original input 中文 🙂');
      expect(input.attachments, [attachment]);
      expect(input.references, [reference]);
      expect(input.canonical, isFalse);
      expect(jsonEncode([page, run]), before);
    }
  });

  test('canonical rows replace the capture once', () {
    final run = {'turn': 'turn', 'input': capture};
    final page = {
      'entries': [
        {
          'turn': 'other',
          'author': 'user',
          'parts': [
            {'kind': 'text', 'data': 'Another turn'},
          ],
        },
        {
          'turn': 'turn',
          'author': 'user',
          'parts': [
            {'kind': 'text', 'data': 'Canonical input'},
            {'kind': 'attachment', 'data': attachment},
            {'kind': 'reference', 'data': reference},
          ],
        },
      ],
    };
    final before = jsonEncode([page, run]);
    final input = turnInputs(page, run).single;
    expect(input.text, 'Canonical input');
    expect(input.attachments, [attachment]);
    expect(input.references, [reference]);
    expect(input.canonical, isTrue);
    expect(jsonEncode([page, run]), before);
  });

  test('empty canonical content still replaces captured text', () {
    final page = {
      'entries': [
        {
          'turn': 'turn',
          'author': 'user',
          'parts': [
            {'kind': 'attachment', 'data': attachment},
          ],
        },
      ],
    };
    final input = turnInputs(page, {'turn': 'turn', 'input': capture}).single;
    expect(input.text, isEmpty);
    expect(input.attachments, [attachment]);
    expect(input.canonical, isTrue);
    expect(turnInputs({}, {'turn': 'turn'}), isEmpty);
    expect(
      turnInputs(page, {'turn': 'turn', 'notice': true, 'input': capture}),
      isEmpty,
    );
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
