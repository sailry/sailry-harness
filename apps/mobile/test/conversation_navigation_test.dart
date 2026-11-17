import 'dart:async';

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/conversation_frame.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> conversation({
  String title = 'Conversation',
  String turn = 'first',
  int revision = 1,
  int attention = 1,
  bool unread = true,
}) => {
  'id': 'session',
  'title': title,
  'revision': revision,
  'activity': {
    'title': title,
    'attention': {'revision': attention, 'unread': unread},
    'run': {'turn': turn, 'sequence': 1, 'revision': 1, 'status': 'completed'},
  },
};

Map<String, dynamic> preview(String text) => {
  'data': {
    'page': {
      'revision': 1,
      'entries': [
        {
          'parts': [
            {'kind': 'text', 'data': text},
          ],
        },
      ],
    },
  },
};

Widget screen(
  AppSession session, {
  Widget home = const TasksPage(),
  GlobalKey<NavigatorState>? navigator,
}) => SessionScope(
  session: session,
  child: MaterialApp(
    navigatorKey: navigator,
    theme: SailryTheme.of(Brightness.light),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: home,
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

void main() {
  testWidgets('read acknowledgements and titles do not reload history', (
    tester,
  ) async {
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        reads++;
        return preview('Preview $reads');
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(screen(session));
    await tester.pump();
    expect(reads, 1);

    host.snapshot['sessions'] = [
      conversation(title: 'Renamed', revision: 2, attention: 2, unread: false),
    ];
    host.notifyListeners();
    await tester.pump();
    expect(find.text('Renamed'), findsOneWidget);
    expect(find.text('Preview 1'), findsOneWidget);
    expect(reads, 1);

    host.snapshot['sessions'] = [conversation(turn: 'second')];
    host.notifyListeners();
    await tester.pump();
    await tester.pump();
    expect(reads, 2);
    expect(find.text('Preview 2'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('covered tasks resume once after the return transition', (
    tester,
  ) async {
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        reads++;
        return preview('Preview $reads');
      },
    );
    final session = AppSession.test(hosts: [host]);
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(screen(session, navigator: navigator));
    await tester.pumpAndSettle();
    final previous = tester.widget<ConversationTask>(
      find.byType(ConversationTask),
    );
    unawaited(
      navigator.currentState!.push(
        CupertinoPageRoute<void>(
          builder: (_) => const Scaffold(body: Text('Details')),
        ),
      ),
    );
    await tester.pumpAndSettle();

    for (final turn in ['second', 'third']) {
      host.snapshot['sessions'] = [
        conversation(title: 'Updated $turn', turn: turn),
      ];
      host.notifyListeners();
      await tester.pump();
    }
    expect(reads, 1);
    expect(
      tester.widget<ConversationTask>(
        find.byType(ConversationTask, skipOffstage: false),
      ),
      same(previous),
    );

    navigator.currentState!.pop();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(reads, 1);
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.text('Updated third'), findsOneWidget);
    expect(find.text('Preview 2'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('a covered preview stops following large tool history', (
    tester,
  ) async {
    final pending = Completer<Map<String, dynamic>>();
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        return ++reads == 1 ? pending.future : preview('Latest message');
      },
    );
    final session = AppSession.test(hosts: [host]);
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(screen(session, navigator: navigator));
    await tester.pump();
    unawaited(
      navigator.currentState!.push(
        CupertinoPageRoute<void>(
          builder: (_) => const Scaffold(body: Text('Details')),
        ),
      ),
    );
    await tester.pumpAndSettle();
    pending.complete({
      'data': {
        'page': {'revision': 1, 'entries': <dynamic>[]},
        'missing': ['first'],
      },
    });
    await tester.pump();
    expect(reads, 1);

    navigator.currentState!.pop();
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.text('Latest message'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('returning to tasks applies the resource tab global selection', (
    tester,
  ) async {
    var firstReads = 0;
    var secondReads = 0;
    final first = HostConnection.test(
      id: 'first',
      label: 'First host',
      snapshot: {
        'sessions': [conversation(title: 'First host conversation')],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        firstReads++;
        return preview('First preview');
      },
    );
    final second = HostConnection.test(
      id: 'second',
      label: 'Second host',
      snapshot: {
        'projects': [
          {'id': 'target', 'name': 'Target project'},
          {'id': 'other', 'name': 'Other project'},
        ],
        'worktrees': [
          {
            'id': 'target-tree',
            'project': 'target',
            'path': '/target',
            'main': true,
          },
          {
            'id': 'other-tree',
            'project': 'other',
            'path': '/other',
            'main': true,
          },
        ],
        'sessions': [
          {
            ...conversation(title: 'Selected project conversation'),
            'project': 'target',
          },
          {
            ...conversation(title: 'Other project conversation'),
            'id': 'other-session',
            'project': 'other',
          },
        ],
      },
      command: (kind, data) async {
        if (kind == 'read_conversation') {
          expect(data?['session'], 'session');
          secondReads++;
          return preview('Second preview');
        }
        expect(kind, 'inspect_git');
        return {
          'data': {'kind': 'directory'},
        };
      },
    );
    final session = AppSession.test(hosts: [first, second]);
    var tab = 0;
    await tester.pumpWidget(
      screen(
        session,
        home: StatefulBuilder(
          builder: (context, setState) => Scaffold(
            body: IndexedStack(
              index: tab,
              children: [
                TickerMode(enabled: tab == 0, child: const TasksPage()),
                TickerMode(enabled: tab == 1, child: const ResourcesPage()),
              ],
            ),
            bottomNavigationBar: Row(
              children: [
                TextButton(
                  onPressed: () => setState(() => tab = 0),
                  child: const Text('Tasks tab'),
                ),
                TextButton(
                  onPressed: () => setState(() => tab = 1),
                  child: const Text('Resources tab'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(firstReads, 1);
    expect(secondReads, 0);
    final previous = tester.widget<ConversationTask>(
      find.byType(ConversationTask),
    );

    await tester.tap(find.text('Resources tab'));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byTooltip(
        tester.element(find.byType(ResourcesPage)).tr('selectHost'),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('host-row-second')));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(ListTile, 'Target project'));
    await tester.pumpAndSettle();
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, 'target');
    expect(session.selectedWorktreeId, 'target-tree');
    expect(firstReads, 1);
    expect(secondReads, 0);
    expect(
      tester.widget<ConversationTask>(
        find.byType(ConversationTask, skipOffstage: false),
      ),
      same(previous),
    );

    await tester.tap(find.text('Tasks tab'));
    await tester.pumpAndSettle();
    expect(find.text('First host conversation'), findsNothing);
    expect(find.text('Other project conversation'), findsNothing);
    expect(find.text('Selected project conversation'), findsOneWidget);
    expect(find.text('Second preview'), findsOneWidget);
    expect(
      tester.widget<ConversationTask>(find.byType(ConversationTask)).host,
      same(second),
    );
    expect(firstReads, 1);
    expect(secondReads, 1);
    await release(tester, session);
  });

  testWidgets('conversation details ignore unrelated host notifications', (
    tester,
  ) async {
    final current = HostConnection.test(
      id: 'current',
      label: 'Current',
      connected: false,
      snapshot: {
        'sessions': [conversation()],
      },
    );
    final other = HostConnection.test(
      id: 'other',
      label: 'Other',
      connected: false,
    );
    final session = AppSession.test(hosts: [current, other]);
    await tester.pumpWidget(
      screen(
        session,
        home: LiveConversationPage(
          host: current,
          sessionId: 'session',
          initialSession: conversation(),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final previous = tester.widget<ConversationFrame>(
      find.byType(ConversationFrame),
    );
    other.notifyListeners();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      same(previous),
    );
    session.selectHost(other.id);
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      same(previous),
    );
    expect(
      tester
          .widget<LiveConversationPage>(find.byType(LiveConversationPage))
          .host,
      same(current),
    );
    current.snapshot['sessions'] = [conversation(title: 'Own host update')];
    current.notifyListeners();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)).title,
      'Own host update',
    );
    final beforeClose = tester.widget<ConversationFrame>(
      find.byType(ConversationFrame),
    );
    await current.close();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      isNot(same(beforeClose)),
    );
    await release(tester, session);
  });
}
