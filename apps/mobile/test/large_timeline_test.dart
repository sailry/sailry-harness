import 'dart:collection';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';
import 'package:sailry_mobile/runtime/session.dart';

class _CountedRun extends MapView<String, dynamic> {
  _CountedRun(String turn)
    : super({'turn': turn, 'kind': 'task', 'status': 'completed'});

  int reads = 0;

  @override
  dynamic operator [](Object? key) {
    reads++;
    return super[key];
  }
}

void main() {
  testWidgets(
    'opening and rebuilding a large timeline only visits nearby turns',
    (tester) async {
      const size = 2048;
      final runs = [for (var i = 0; i < size; i++) _CountedRun('t$i')];
      final index = TranscriptIndex({
        'timeline': [for (var i = 0; i < size; i++) 't$i'],
        'snapshot': {
          'page': {
            'runs': runs,
            'entries': [
              for (var i = 0; i < size; i++)
                {
                  'id': 'e$i',
                  'turn': 't$i',
                  'author': 'assistant',
                  'parts': [
                    {'kind': 'text', 'data': 'Reply $i'},
                  ],
                },
            ],
          },
        },
        'segments': {
          for (var i = 0; i < size; i++)
            't$i': [
              {
                'rows': [
                  {
                    'source': {'entry': 'e$i', 'index': 0},
                    'end': 1,
                    'text_offset': 0,
                    'text_last': true,
                  },
                ],
              },
            ],
        },
      });
      expect(index.timeline.last, same(runs.last));
      final host = HostConnection.test(
        id: 'node',
        label: 'Test host',
        snapshot: const {},
        command: (kind, _) async => fail('Unexpected command: $kind'),
      );
      final scroll = ScrollController();
      addTearDown(host.dispose);
      addTearDown(scroll.dispose);

      for (final inset in [0.0, 12.0]) {
        for (final run in runs) {
          run.reads = 0;
        }
        await tester.pumpWidget(
          MaterialApp(
            home: Scaffold(
              body: LiveTimeline(
                transcript: index,
                connected: true,
                controller: scroll,
                padding: EdgeInsets.only(top: inset),
                session: const {'id': 'session'},
                host: host,
                command: (kind, _) async => fail('Unexpected command: $kind'),
              ),
            ),
          ),
        );
        expect(runs.last.reads, greaterThan(0));
        expect(runs.first.reads, 0);
        expect(runs.where((run) => run.reads != 0).length, lessThan(64));
        expect(scroll.offset, 0);
      }
    },
  );
}
