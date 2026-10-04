import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/search.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'live_conversations_test.dart' as fixture;

Map<String, dynamic> match({String turn = 'old'}) => {
  'turn': turn,
  'turn_sequence': 1,
  'entry': 'old-user',
  'sequence': 2,
  'part': 0,
  'author': 'user',
  'snippet': 'A 中文🙂 result',
  'highlight': {'start': 2, 'end': 12},
};
Map<String, dynamic> results({
  int revision = 12,
  Object? next,
  List<Map<String, dynamic>>? matches,
}) => {
  'Ok': {
    'session': 'session',
    'revision': revision,
    'matches': matches ?? [match()],
    'next_before': next,
  },
};
Map<String, dynamic> history({bool older = false, int revision = 12}) {
  final value = fixture.view();
  final page = value['snapshot']['page'] as Map;
  page['revision'] = revision;
  if (!older) page['next_before'] = 20;
  final ids = [if (older) 'old', for (var i = 0; i < 20; i++) 'turn-$i'];
  page['runs'] = [
    for (final id in ids) {'turn': id, 'status': 'completed'},
  ];
  page['entries'] = [
    for (final id in ids) ...[
      {
        ...fixture.entry(
          '$id-user',
          'user',
          id == 'old' ? 'A 中文🙂 result' : 'Question $id',
        ),
        'turn': id,
      },
      {...fixture.entry('$id-answer', 'assistant', 'Answer $id'), 'turn': id},
    ],
  ];
  return value;
}

class SearchUpdates extends fixture.UpdatesFixture {
  final targets = <BigInt>[];
  @override
  Future<void> loadThrough({required BigInt sequence}) async {
    targets.add(sequence);
    emit(history(older: true));
  }
}

class SearchConnection extends fixture.ConnectionFixture {
  SearchConnection() {
    updates = SearchUpdates();
  }
  final queries = <Map<String, dynamic>>[];
  Future<Map<String, dynamic>> Function(Map<String, dynamic>)? search;
  @override
  Future<String> searchConversation({
    required String session,
    required String query,
  }) async {
    expect(session, 'session');
    final value = jsonDecode(query) as Map<String, dynamic>;
    queries.add(value);
    return jsonEncode(search == null ? results() : await search!(value));
  }
}

Future<AppSession> mount(
  WidgetTester tester,
  SearchConnection connection, {
  bool conversation = false,
}) async {
  final record = fixture.session();
  final host = HostConnection.test(
    id: 'node',
    label: 'Node',
    connection: connection,
    snapshot: {
      'sessions': [record],
    },
    command: (_, _) async =>
        throw StateError('search must use the shared query'),
  );
  final app = AppSession.test(hosts: [host]);
  await fixture.mount(
    tester,
    conversation
        ? LiveConversationPage(
            host: host,
            sessionId: 'session',
            initialSession: record,
          )
        : ConversationSearch(host: host, session: 'session'),
    app,
  );
  return app;
}

Future<void> close(WidgetTester tester, AppSession app) async {
  await tester.pump(const Duration(seconds: 3));
  await tester.pumpWidget(const SizedBox());
  app.dispose();
}

void main() {
  testWidgets('manual search coalesces requests and highlights Unicode bytes', (
    tester,
  ) async {
    final connection = SearchConnection();
    final gate = Completer<Map<String, dynamic>>();
    connection.search = (_) => gate.future;
    final app = await mount(tester, connection);
    await tester.enterText(find.byType(TextField), ' 中文🙂 ');
    await tester.pump(const Duration(seconds: 1));
    expect(connection.queries, isEmpty);
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pump();
    expect(connection.queries, [
      {'text': '中文🙂', 'case_sensitive': false, 'before': null, 'limit': 30},
    ]);
    expect(
      tester
          .widget<IconButton>(
            find.byWidgetPredicate(
              (widget) =>
                  widget is IconButton && widget.tooltip == tr('messageSearch'),
            ),
          )
          .onPressed,
      isNull,
    );
    gate.complete(results());
    await tester.pumpAndSettle();
    final rich = tester.widget<Text>(
      find.descendant(
        of: find.byType(SearchSnippet),
        matching: find.byType(Text),
      ),
    );
    expect(rich.textSpan!.toPlainText(), 'A 中文🙂 result');
    expect((rich.textSpan! as TextSpan).children![1].toPlainText(), '中文🙂');
    expect(tester.takeException(), isNull);
    await close(tester, app);
  });

  testWidgets('pagination rejects changed history and restarts explicitly', (
    tester,
  ) async {
    final connection = SearchConnection();
    connection.search = (query) async =>
        query['before'] == null ? results(next: 9) : results(revision: 13);
    final app = await mount(tester, connection);
    await tester.enterText(find.byType(TextField), 'result');
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('resourceMore')));
    await tester.pumpAndSettle();
    expect(connection.queries.last['before'], 9);
    expect(find.text(tr('messageSearchStale')), findsOneWidget);
    expect(find.byType(SearchSnippet), findsOneWidget);
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    expect(connection.queries.last['before'], isNull);
    expect(find.text(tr('messageSearchStale')), findsNothing);
    expect(find.byType(SearchSnippet), findsOneWidget);
    await close(tester, app);
  });

  testWidgets(
    'selecting an old hit loads through once and scrolls to its turn',
    (tester) async {
      final connection = SearchConnection();
      final app = await mount(tester, connection, conversation: true);
      connection.updates.emit(history());
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('messageSearch')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), '中文🙂');
      await tester.tap(find.byTooltip(tr('messageSearch')));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(SearchSnippet));
      await tester.pumpAndSettle();
      expect((connection.updates as SearchUpdates).targets, [BigInt.one]);
      expect(find.byType(ConversationSearch), findsNothing);
      await tester.pump();
      await tester.pumpAndSettle();
      final bounds = tester.getRect(find.text('A 中文🙂 result'));
      expect(bounds.top, greaterThan(50));
      expect(bounds.bottom, lessThan(800));
      connection.updates.emit(history(older: true));
      await tester.pumpAndSettle();
      expect((connection.updates as SearchUpdates).targets, [BigInt.one]);
      expect(tester.takeException(), isNull);
      await close(tester, app);
    },
  );

  testWidgets('an earlier paging failure does not hide an already loaded hit', (
    tester,
  ) async {
    final connection = SearchConnection();
    connection.search = (_) async => results(matches: [match(turn: 'turn-0')]);
    final app = await mount(tester, connection, conversation: true);
    connection.updates.emit({
      ...history(),
      'older_error': {'code': 'unavailable'},
    });
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'result');
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(SearchSnippet));
    await tester.pumpAndSettle();
    expect((connection.updates as SearchUpdates).targets, isEmpty);
    final bounds = tester.getRect(find.text('Question turn-0'));
    expect(bounds.top, greaterThan(50));
    expect(bounds.bottom, lessThan(800));
    expect(find.text(tr('messageSearchStale')), findsNothing);
    await close(tester, app);
  });

  testWidgets('stale search results cannot navigate changed history', (
    tester,
  ) async {
    final connection = SearchConnection();
    final app = await mount(tester, connection, conversation: true);
    connection.updates.emit(history(revision: 13));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'result');
    await tester.tap(find.byTooltip(tr('messageSearch')));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(SearchSnippet));
    await tester.pumpAndSettle();
    expect((connection.updates as SearchUpdates).targets, isEmpty);
    expect(find.text(tr('messageSearchStale')), findsOneWidget);
    await close(tester, app);
  });
}
