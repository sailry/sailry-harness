import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/command_result.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/tool_content.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  test('shared visibility controls activity rows', () {
    Map<String, dynamic> entry(String id) => {
      'id': id,
      'author': 'assistant',
      'parts': [
        {
          'kind': 'tool_call',
          'data': {'arguments': <String, dynamic>{}},
        },
      ],
    };
    final entries = [
      entry('hidden'),
      entry('summary'),
      entry('failure'),
      entry('image'),
      {
        'id': 'response',
        'author': 'assistant',
        'parts': [
          {
            'kind': 'tool_result',
            'data': {
              'result': null,
              'images': [
                {'part': 0, 'index': 0, 'attachment': <String, dynamic>{}},
              ],
            },
          },
        ],
      },
    ];
    final calls = [
      for (final id in ['hidden', 'summary', 'failure', 'image'])
        {
          'source': {'entry': id, 'index': 0},
          'hidden': id == 'hidden',
          'details': id == 'failure' || id == 'image',
        },
    ];
    final before = jsonEncode(entries);
    final visible = visibleParts(entries, calls);
    expect(visible.map((row) => row.$1['id']), [
      'summary',
      'failure',
      'image',
      'response',
    ]);
    expect(visible.last.$3['kind'], 'image');
    expect(jsonEncode(entries), before);
  });

  test('business fields never establish execution failure', () {
    final call = {
      'failure': null,
      'result': {
        'isError': false,
        'structuredContent': {'isError': true, 'error': 'domain row'},
      },
      'resolved': {
        'diagnostics': [
          {'text': 'Declared diagnostic', 'error': true},
        ],
        'command': {'kind': 'signal', 'data': 9},
      },
    };
    final before = jsonEncode(call);
    expect(toolFailed(call), isFalse);
    expect(jsonEncode(call), before);
    expect(toolFailed({'failure': <String, dynamic>{}}), isTrue);
    for (final code in ['cancelled', 'outcome_unknown']) {
      expect(
        toolFailed({
          'failure': {'code': code},
        }),
        isFalse,
      );
    }
  });

  test('prepared output determines the existing disclosure body', () {
    expect(hasToolContent({'details': false}), isFalse);
    expect(
      hasToolContent({
        'output': {
          'preview': 'Batch edit preview',
          'body': {'text': '', 'empty': null},
        },
      }),
      isTrue,
    );
    expect(hasToolContent({'output_deferred': true}), isTrue);
    expect(
      hasToolContent({
        'content': {
          'blocks': [
            {'kind': 'image', 'index': 0},
            {'kind': 'file', 'path': 'report.txt'},
          ],
        },
      }),
      isFalse,
    );
    expect(
      hasToolContent({
        'content': {
          'blocks': [
            {'kind': 'text'},
          ],
        },
        'texts': ['Original literal'],
      }),
      isTrue,
    );
  });

  test('selected command outcomes keep precise text', () {
    expect(
      commandStatus({'kind': 'exited', 'data': 7}),
      tr('toolExitCode').replaceAll('{code}', '7'),
    );
    expect(
      commandStatus({'kind': 'signal', 'data': 9}),
      tr('toolSignal').replaceAll('{signal}', '9'),
    );
    expect(commandStatus({'kind': 'timed_out'}), tr('toolTimedOut'));
    expect(commandStatus({'kind': 'cancelled'}), tr('toolCancelled'));
    expect(
      commandStatus({'kind': 'unknown', 'data': 'Original reason'}),
      tr('toolOutcomeUnknown'),
    );
    expect(commandStatus({}), isNull);
  });
}
