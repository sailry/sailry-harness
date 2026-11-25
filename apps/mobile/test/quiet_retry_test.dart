import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

TranscriptIndex _running(Map<String, dynamic> retry) => TranscriptIndex({
  'connected': true,
  'timeline': ['t1'],
  'snapshot': {
    'page': {
      'runs': [
        {'turn': 't1', 'kind': 'task', 'status': 'running'},
      ],
      'entries': [
        {
          'id': 'entry-t1',
          'turn': 't1',
          'author': 'assistant',
          'parts': [
            {'kind': 'resource', 'data': retry},
          ],
        },
      ],
    },
  },
  'segments': {
    't1': [
      {
        'rows': [
          {
            'source': {'entry': 'entry-t1', 'index': 0},
            'end': 1,
          },
        ],
      },
    ],
  },
});

void main() {
  for (final (retry, thinking) in [
    ({'type': 'model_retry', 'attempt': 0, 'limit': 9}, true),
    ({'type': 'model_retry', 'attempt': 0, 'limit': 9, 'delay_ms': 5000}, true),
    ({'type': 'model_retry', 'attempt': 0, 'limit': 9, 'delay_ms': 6000}, true),
    (
      {'type': 'model_retry', 'attempt': 1, 'limit': 9, 'delay_ms': 6000},
      false,
    ),
    (
      {'type': 'model_retry', 'attempt': 2, 'limit': 9, 'delay_ms': 6000},
      false,
    ),
    (
      {'type': 'model_retry', 'attempt': 9, 'limit': 9, 'delay_ms': 30000},
      false,
    ),
    (
      {'type': 'model_retry', 'attempt': 1, 'limit': 9, 'delay_ms': 5000},
      false,
    ),
    ({'type': 'model_retry', 'attempt': 10, 'limit': 10}, false),
  ]) {
    testWidgets('a retry with attempt ${retry['attempt']} '
        'and delay ${retry['delay_ms'] ?? 0} '
        '${thinking ? 'keeps' : 'replaces'} the thinking phase', (
      tester,
    ) async {
      final host = HostConnection.test(
        id: 'node',
        label: 'Test host',
        snapshot: const {},
        command: (kind, _) async => fail('Unexpected command: $kind'),
      );
      final scroll = ScrollController();
      addTearDown(host.dispose);
      addTearDown(scroll.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: LiveTimeline(
              transcript: _running(retry),
              connected: true,
              controller: scroll,
              padding: EdgeInsets.zero,
              session: const {'id': 'session'},
              host: host,
              command: (kind, _) async => fail('Unexpected command: $kind'),
            ),
          ),
        ),
      );
      await tester.pump(const Duration(milliseconds: 100));
      expect(
        find.textContaining(tr('thinkingNow')),
        thinking ? findsOneWidget : findsNothing,
      );
      expect(
        find.text(
          tr('conversationModelRetrying')
              .replaceAll('{attempt}', '${retry['attempt']}')
              .replaceAll('{limit}', '${retry['limit']}'),
        ),
        thinking ? findsNothing : findsOneWidget,
      );
    });
  }
}
