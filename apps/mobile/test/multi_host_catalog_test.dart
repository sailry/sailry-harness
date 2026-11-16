import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/conversations/live/task_filters.dart';
import 'package:sailry_mobile/features/conversations/live/tasks.dart';
import 'package:sailry_mobile/features/conversations/live/terminal_tasks.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/catalog.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/swipe_action.dart';

Map<String, dynamic> project(String name) => {
  'id': 'shared-id',
  'name': name,
  'path': '/$name',
};

Map<String, dynamic> conversation(
  String title, {
  String? project = 'shared-id',
}) => {
  'id': 'same-session',
  'project': project,
  'revision': 1,
  'activity': {'title': title},
};

HostConnection source(
  String id, {
  bool connected = true,
  CommandHandler? command,
}) => HostConnection.test(
  id: id,
  label: id,
  connected: connected,
  snapshot: {
    'projects': [project('$id-project')],
    'sessions': [conversation('$id-conversation')],
    'worktrees': [
      {'id': '$id-tree', 'project': 'shared-id', 'path': '/$id-project'},
    ],
  },
  command: command ?? (_, _) async => {'data': <String, dynamic>{}},
);

Widget screen(AppSession session, Widget child) => SessionScope(
  session: session,
  child: MaterialApp(
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: child,
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

Finder slidable(Object identity) => find.byWidgetPredicate(
  (widget) =>
      widget is Slidable &&
      widget.key is ValueKey &&
      (widget.key as ValueKey).value == identity,
);

void main() {
  test('catalog identity includes host and retains offline records', () async {
    final first = source('first', connected: false);
    final second = source('second');
    first.snapshot['projects'] = [
      project('first-project'),
      project('duplicate'),
    ];
    final values = hostRecords([first, first, second], 'projects');
    expect(values.length, 2);
    expect(values[0].$1, same(first));
    expect(values[0].$2['name'], 'first-project');
    expect(values[1].$1, same(second));
    expect(values[1].$2['name'], 'second-project');
    await first.close();
    await second.close();
    first.dispose();
    second.dispose();
  });

  testWidgets('new projects explicitly share on their owning host', (
    tester,
  ) async {
    Map<String, dynamic>? requested;
    final host = source(
      'project-host',
      command: (kind, data) async {
        expect(kind, 'register_project');
        requested = data;
        return {'data': <String, dynamic>{}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(
      screen(session, Scaffold(body: ProjectForm(host: host))),
    );
    await tester.enterText(find.byType(TextField).at(0), 'Shared project');
    await tester.enterText(find.byType(TextField).at(1), '/shared-project');
    await tester.tap(find.byType(FilledButton));
    await tester.pumpAndSettle();
    expect(requested, {
      'name': 'Shared project',
      'path': '/shared-project',
      'visibility': 'shared',
    });
    await release(tester, session);
  });

  testWidgets('tasks aggregate hosts and route row actions to their owner', (
    tester,
  ) async {
    final firstCalls = <String>[];
    final secondCalls = <String>[];
    final first = source(
      'first',
      connected: false,
      command: (kind, _) async {
        firstCalls.add(kind);
        return {'data': <String, dynamic>{}};
      },
    );
    final second = source(
      'second',
      command: (kind, _) async {
        secondCalls.add(kind);
        return {'data': <String, dynamic>{}};
      },
    );
    final session = AppSession.test(hosts: [first, second]);
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    expect(find.text('first-conversation'), findsOneWidget);
    expect(find.text('second-conversation'), findsOneWidget);
    expect(firstCalls, isEmpty);
    expect(secondCalls, ['read_conversation']);
    expect(tester.widget<PageFrame>(find.byType(PageFrame)).failure, isNull);
    final row = tester.widget<Slidable>(slidable(('second', 'same-session')));
    final archive = row.endActionPane!.children.first as SwipeAction;
    archive.onPressed!(tester.element(find.byType(LiveTasksPage)));
    await tester.pump();
    expect(secondCalls.last, 'set_session_archived');
    expect(firstCalls, isEmpty);
    expect(session.selectedHost, same(first));
    final offline = tester.widget<Slidable>(
      slidable(('first', 'same-session')),
    );
    expect(
      (offline.endActionPane!.children.first as SwipeAction).onPressed,
      isNull,
    );
    await release(tester, session);
  });

  testWidgets(
    'project filters include host identity and select action context',
    (tester) async {
      final first = source('first');
      final second = source('second');
      final session = AppSession.test(hosts: [first, second]);
      await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
      await tester.pump();
      await tester.tap(find.byType(ProjectBar));
      await tester.pumpAndSettle();
      await tester.tap(find.text('second-project'));
      await tester.pumpAndSettle();
      expect(find.text('first-conversation'), findsNothing);
      expect(find.text('second-conversation'), findsOneWidget);
      expect(
        tester
            .widget<ConversationTask>(find.byType(ConversationTask))
            .project['name'],
        'second-project',
      );
      expect(session.selectedHost, same(first));
      await tester.tap(find.byType(ProjectBar));
      await tester.pumpAndSettle();
      await tester.tap(
        find.text(
          tester.element(find.byType(LiveTasksPage)).tr('hostRegisterProject'),
        ),
      );
      await tester.pumpAndSettle();
      expect(
        tester.widget<ProjectForm>(find.byType(ProjectForm)).host,
        same(second),
      );
      await release(tester, session);
    },
  );

  testWidgets('offline cached tasks stay visible and counts aggregate', (
    tester,
  ) async {
    final first = source('first', connected: false);
    final second = source('second', connected: false);
    second.snapshot['sessions'] = [
      {...conversation('second-conversation'), 'archived': true},
    ];
    final session = AppSession.test(hosts: [first, second]);
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    expect(find.text('first-conversation'), findsOneWidget);
    expect(tester.widget<PageFrame>(find.byType(PageFrame)).failure, isNull);
    final filters = tester.widget<TaskFilters>(find.byType(TaskFilters));
    expect(filters.archived, 1);
    filters.onArchive();
    await tester.pump();
    expect(find.text('second-conversation'), findsOneWidget);
    expect(find.text('first-conversation'), findsNothing);
    await release(tester, session);
  });

  testWidgets('terminal records route close actions to their host', (
    tester,
  ) async {
    final calls = <String>[];
    final first = source('first');
    final second = source(
      'second',
      command: (kind, data) async {
        calls.add(kind);
        if (kind == 'close_terminal') {
          expect(data?['worktree'], 'second-tree');
          expect(data?['terminal'], 'same-terminal');
        }
        return {'data': <String, dynamic>{}};
      },
    );
    for (final host in [first, second]) {
      host.snapshot['terminals'] = [
        {
          'id': 'same-terminal',
          'title': '${host.id}-terminal',
          'worktree': '${host.id}-tree',
          'status': {'kind': 'running'},
        },
      ];
    }
    final session = AppSession.test(hosts: [first, second]);
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    final rows = tester
        .widgetList<TerminalTask>(find.byType(TerminalTask))
        .toList();
    expect(rows.length, 2);
    expect(rows[0].host, same(first));
    expect(rows[1].host, same(second));
    final terminal = tester.widget<Slidable>(
      slidable(('terminal', 'second', 'same-terminal')),
    );
    final close = terminal.endActionPane!.children.single as SwipeAction;
    close.onPressed!(tester.element(find.byType(LiveTasksPage)));
    await tester.pump();
    expect(calls.last, 'close_terminal');
    await release(tester, session);
  });

  testWidgets('projects aggregate and open the owning host workspace', (
    tester,
  ) async {
    final first = source('first', connected: false);
    final second = source('second', connected: false);
    final session = AppSession.test(hosts: [first, second]);
    await tester.pumpWidget(screen(session, ProjectsPage(host: first)));
    await tester.pump();
    expect(find.text('first-project'), findsOneWidget);
    expect(find.text('second-project'), findsOneWidget);
    expect(tester.widget<PageFrame>(find.byType(PageFrame)).failure, isNull);
    await tester.tap(find.text('second-project'));
    await tester.pumpAndSettle();
    final resource = tester.widget<ResourcesPage>(find.byType(ResourcesPage));
    expect(resource.hostId, second.id);
    expect(resource.worktreeId, 'second-tree');
    expect(session.selectedHost, same(first));
    await release(tester, session);
  });
}
