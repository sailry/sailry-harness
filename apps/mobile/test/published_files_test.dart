import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/files.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'live_conversations_test.dart' as fixture;

Map<String, dynamic> publication(String name, String path, {int size = 100}) =>
    {
      'kind': 'file',
      'name': name,
      'path': path,
      'mime': 'text/markdown',
      'size': size,
      'revision': 'ab12',
    };

Map<String, dynamic> call(String id, Map<String, dynamic> file) => {
  'name': id,
  'turn': 'turn',
  'state': 'returned',
  'presentation': 'content',
  'source': {'entry': id, 'index': 0},
  'response': {'entry': id, 'index': 1},
  'content': {
    'version': 1,
    'blocks': [file],
  },
};

Map<String, dynamic> entry(String id) => {
  'id': id,
  'turn': 'turn',
  'author': 'assistant',
  'parts': [
    {
      'kind': 'tool_call',
      'data': {'arguments': {}},
    },
    {
      'kind': 'tool_result',
      'data': {'result': {}, 'images': []},
    },
  ],
};

void main() {
  test('retains latest metadata within the same turn', () {
    final first = publication('First', 'first.md');
    final old = publication('Old guide', 'guide.md');
    final latest = publication('Published guide', 'guide.md', size: 2400);
    expect(
      publishedFiles([
        call('first', first),
        call('old', old),
        call('latest', latest),
        {...call('other', old), 'turn': 'another-turn'},
      ], 'turn'),
      [first, latest],
    );
  });

  for (final connected in [true, false]) {
    testWidgets('published cards remain visible when connected $connected', (
      tester,
    ) async {
      final reads = <Map<String, dynamic>?>[];
      final downloads = <Map<String, dynamic>?>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        command: (kind, data) async {
          if (kind == 'download_file') {
            downloads.add(data);
            throw const CommandFailure('unavailable');
          }
          expect(kind, 'read_file');
          reads.add(data);
          return {
            'data': {'text': '# Stored artifact', 'revision': 'ab12'},
          };
        },
      );
      final app = AppSession.test(hosts: [host]);
      addTearDown(app.dispose);
      final view = fixture.view(
        connected: connected,
        entries: [
          entry('old'),
          entry('latest'),
          fixture.entry('answer', 'assistant', 'Here is the guide'),
        ],
        calls: [
          call('old', publication('Old guide', 'guide.md')),
          call(
            'latest',
            publication('Published guide', 'guide.md', size: 2400),
          ),
        ],
      );
      view['snapshot']['page']['runs'][0]['worktree'] = 'captured';
      await fixture.mount(
        tester,
        Scaffold(
          body: SingleChildScrollView(
            child: LiveTimeline(
              view: view,
              session: fixture.session(),
              host: host,
              command: (_, _) async => {},
            ),
          ),
        ),
        app,
      );
      await tester.pumpAndSettle();
      expect(find.text('Published guide'), findsOneWidget);
      expect(find.text('Old guide'), findsNothing);
      expect(find.text('text/markdown · 2.4 KB'), findsOneWidget);
      expect(reads, isEmpty);
      expect(
        tester.getTopLeft(find.text('Published guide')).dy,
        greaterThan(tester.getBottomLeft(find.text('Here is the guide')).dy),
      );
      final tile = tester.widget<ListTile>(
        find.descendant(
          of: find.byType(PublishedFiles),
          matching: find.byType(ListTile),
        ),
      );
      if (connected) {
        expect(tile.onTap, isNotNull);
        await tester.tap(find.byTooltip(tr('more')));
        await tester.pumpAndSettle();
        expect(find.text(tr('preview')), findsOneWidget);
        await tester.tap(find.text(tr('save')));
        await tester.pumpAndSettle();
        expect(downloads, [
          {'worktree': 'captured', 'path': 'guide.md'},
        ]);
        expect(find.text(tr('fileSaveFailed')), findsOneWidget);
        await tester.pump(const Duration(seconds: 3));
        await tester.pumpAndSettle();
        await tester.tap(find.text('Published guide'));
        await tester.pumpAndSettle();
        expect(reads, [
          {'worktree': 'captured', 'path': 'guide.md'},
        ]);
        expect(find.text('Stored artifact'), findsOneWidget);
      } else {
        expect(tile.onTap, isNull);
      }
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }
}
