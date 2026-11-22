import 'dart:collection';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';
import 'package:sailry_mobile/runtime/session.dart';

class _CountedRun extends MapView<String, dynamic> {
  _CountedRun(this.number)
    : super({'turn': 't$number', 'kind': 'task', 'status': 'completed'});

  final int number;
  String get turn => 't$number';
  int reads = 0;

  @override
  dynamic operator [](Object? key) {
    reads++;
    return super[key];
  }
}

TranscriptIndex _index(List<_CountedRun> runs) => TranscriptIndex({
  'timeline': [for (final run in runs) run.turn],
  'snapshot': {
    'page': {
      'runs': runs,
      'entries': [
        for (final run in runs)
          {
            'id': 'entry-${run.turn}',
            'turn': run.turn,
            'author': 'assistant',
            'parts': [
              {'kind': 'text', 'data': 'Reply ${run.number}'},
            ],
          },
      ],
    },
  },
  'segments': {
    for (final run in runs)
      run.turn: [
        {
          'rows': [
            {
              'source': {'entry': 'entry-${run.turn}', 'index': 0},
              'end': 1,
              'text_offset': 0,
              'text_last': true,
            },
          ],
        },
      ],
  },
});

Future<void> _show(
  WidgetTester tester,
  TranscriptIndex index,
  HostConnection host,
  ScrollController scroll, {
  EdgeInsets padding = EdgeInsets.zero,
  List<Widget> older = const [],
  Key Function(String turn)? turnKey,
}) => tester.pumpWidget(
  MaterialApp(
    home: Scaffold(
      body: LiveTimeline(
        transcript: index,
        connected: true,
        controller: scroll,
        padding: padding,
        older: older,
        session: const {'id': 'session'},
        host: host,
        command: (kind, _) async => fail('Unexpected command: $kind'),
        turnKey: turnKey,
      ),
    ),
  ),
);

void main() {
  testWidgets(
    'opening, rebuilding and jumping to latest only visit nearby turns',
    (tester) async {
      const size = 2048;
      final runs = List.generate(size, _CountedRun.new);
      final index = _index(runs);
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

      void clearReads() {
        for (final run in runs) {
          run.reads = 0;
        }
      }

      void expectLatest() {
        expect(runs.last.reads, greaterThan(0));
        expect(runs.first.reads, 0);
        expect(runs.where((run) => run.reads != 0).length, lessThan(64));
        expect(scroll.offset, 0);
        expect(find.text('Reply ${size - 1}').hitTestable(), findsOneWidget);
      }

      for (final inset in [0.0, 12.0]) {
        clearReads();
        await _show(
          tester,
          index,
          host,
          scroll,
          padding: EdgeInsets.only(top: inset, bottom: 24),
        );
        expectLatest();
      }

      scroll.jumpTo(scroll.position.viewportDimension * 12);
      await tester.pumpAndSettle();
      expect(scroll.offset, greaterThan(scroll.position.viewportDimension));
      expect(find.text('Reply ${size - 1}'), findsNothing);

      clearReads();
      scroll.jumpTo(scroll.position.minScrollExtent);
      await tester.pumpAndSettle();
      expectLatest();
    },
  );

  testWidgets('older pages keep the current anchor and expose older matches', (
    tester,
  ) async {
    final runs = List.generate(48, _CountedRun.new);
    final host = HostConnection.test(id: 'node', label: 'Test', snapshot: {});
    final scroll = ScrollController();
    final keys = <String, GlobalKey>{};
    addTearDown(host.dispose);
    addTearDown(scroll.dispose);

    Key turnKey(String turn) => keys.putIfAbsent(turn, GlobalKey.new);
    await _show(
      tester,
      _index(runs.sublist(24)),
      host,
      scroll,
      turnKey: turnKey,
      older: [const Text('Loading history', key: ValueKey('older'))],
    );
    scroll.jumpTo(scroll.position.maxScrollExtent);
    await tester.pumpAndSettle();
    expect(find.text('Loading history').hitTestable(), findsOneWidget);

    final anchor = keys['t24']!;
    final anchorContext = anchor.currentContext;
    final anchorTop = tester.getTopLeft(find.byKey(anchor));
    final offset = scroll.offset;
    await _show(tester, _index(runs), host, scroll, turnKey: turnKey);
    expect(find.text('Loading history'), findsNothing);
    expect(anchor.currentContext, same(anchorContext));
    expect(tester.getTopLeft(find.byKey(anchor)), anchorTop);
    expect(scroll.offset, offset);

    expect(keys['t12']?.currentContext, isNull);
    scroll.jumpTo(offset + scroll.position.viewportDimension);
    await tester.pumpAndSettle();
    await Scrollable.ensureVisible(keys['t12']!.currentContext!, alignment: .2);
    await tester.pumpAndSettle();
    expect(find.text('Reply 12').hitTestable(), findsOneWidget);
  });
}
