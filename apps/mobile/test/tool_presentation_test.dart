import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/command_result.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';
import 'package:sailry_mobile/features/conversations/live/tool_content.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  test('renders Client rows with their citation offsets', () {
    final view = {
      'snapshot': {
        'page': {
          'entries': [
            {
              'id': 'entry',
              'citations': [
                {'uri': 'https://example.com/source', 'end': 3},
              ],
            },
          ],
        },
        'drafts': [
          {'id': 'draft'},
        ],
      },
      'rows': {
        'turn': [
          {
            'source': {'entry': 'entry', 'index': 2},
            'draft': false,
            'part': {'kind': 'text', 'data': 'B'},
            'text_offset': 2,
            'text_last': true,
            'work': false,
          },
          {
            'source': {'entry': 'draft', 'index': 0},
            'draft': true,
            'part': {'kind': 'thinking', 'data': 'Working'},
            'work': true,
          },
        ],
      },
    };
    final before = jsonEncode(view);
    final rows = displayRows(view, 'turn');
    expect(rows.first.$1['id'], 'entry');
    expect(rows.first.$2, 2);
    expect(rows.first.$3['data'], 'B [1](<https://example.com/source>)');
    expect(rows.first.$3['work'], isFalse);
    expect(rows.last.$1['id'], 'draft');
    expect(rows.last.$3['data'], 'Working');
    expect(rows.last.$3['work'], isTrue);
    expect(displayRows(view, 'other'), isEmpty);
    expect(jsonEncode(view), before);
  });

  test('business fields never establish execution failure', () {
    final call = {
      'row': {'failed': false},
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
    expect(
      toolFailed({
        'row': {'failed': true},
        'failure': <String, dynamic>{},
      }),
      isTrue,
    );
    for (final code in ['cancelled', 'outcome_unknown']) {
      expect(
        toolFailed({
          'row': {'failed': false},
          'failure': {'code': code},
        }),
        isFalse,
      );
    }
  });

  test('prepared output determines the existing disclosure body', () {
    expect(
      hasToolContent({
        'row': {'has_details': false},
        'details': false,
      }),
      isFalse,
    );
    expect(
      hasToolContent({
        'row': {'has_details': true},
        'output': {
          'preview': 'Batch edit preview',
          'body': {'text': '', 'empty': null},
        },
      }),
      isTrue,
    );
    expect(
      hasToolContent({
        'row': {'has_details': true},
        'details': true,
      }),
      isTrue,
    );
    expect(
      hasToolContent({
        'row': {'has_details': false},
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
        'row': {'has_details': true},
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
