import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/conversations/live/create.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/conversations/live/task_filters.dart';
import 'package:sailry_mobile/features/conversations/live/tasks.dart';
import 'package:sailry_mobile/features/conversations/live/terminal_tasks.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
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
  test(
    'directory fault disables cached source and revocation clears it',
    () async {
      final offline = HostConnection.test(
        id: 'entry/offline',
        label: 'Offline',
        snapshot: {
          'projects': [project('Cached')],
        },
        metadata: {
          'source_fault': {'code': 'unavailable', 'message': 'offline'},
        },
      );
      final revoked = HostConnection.test(
        id: 'entry/revoked',
        label: 'Revoked',
        snapshot: {
          'projects': [project('Private')],
        },
        metadata: {
          'source_fault': {'code': 'permission_denied', 'message': 'revoked'},
        },
      );
      expect(offline.connected, isFalse);
      expect((offline.snapshot['projects'] as List).single['name'], 'Cached');
      expect(revoked.connected, isFalse);
      expect(revoked.snapshot, isEmpty);
      await offline.close();
      await revoked.close();
      offline.dispose();
      revoked.dispose();
    },
  );

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

  test(
    'cold offline source uses only its controller directory summary',
    () async {
      final host = HostConnection.test(
        id: 'entry/storage',
        label: 'Storage',
        metadata: {
          'source_fault': {'code': 'unavailable', 'message': 'offline'},
          'resource_source': {
            'revision': 17,
            'projects': [project('Recovered directory')],
            'worktrees': [
              {'id': 'tree', 'project': 'shared-id', 'path': '/recovered'},
            ],
          },
        },
      );
      expect(host.connected, isFalse);
      expect(
        (host.snapshot['projects'] as List).single['name'],
        'Recovered directory',
      );
      expect(host.snapshot['sessions'], isNull);
      await expectLater(
        host.command('register_project'),
        throwsA(isA<CommandFailure>()),
      );
      await host.close();
      host.dispose();
    },
  );

  test('offline directory cannot replace a newer complete snapshot', () async {
    final host = HostConnection.test(
      id: 'entry/storage',
      label: 'Storage',
      snapshot: {
        'cursor': 18,
        'projects': [project('Latest project')],
        'worktrees': [
          {'id': 'new-tree', 'project': 'shared-id', 'path': '/latest'},
        ],
      },
      metadata: {
        'source_fault': {'code': 'unavailable', 'message': 'offline'},
        'resource_source': {
          'revision': 17,
          'projects': [project('Older directory')],
          'worktrees': [
            {'id': 'old-tree', 'project': 'shared-id', 'path': '/older'},
          ],
        },
      },
    );
    expect(host.connected, isFalse);
    expect(
      (host.snapshot['projects'] as List).single['name'],
      'Latest project',
    );
    expect((host.snapshot['worktrees'] as List).single['id'], 'new-tree');
    expect(host.snapshot['cursor'], 18);
    await host.close();
    host.dispose();
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

  testWidgets(
    'tasks follow the selected host and route actions to their owner',
    (tester) async {
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
      expect(find.text('second-conversation'), findsNothing);
      expect(firstCalls, isEmpty);
      expect(secondCalls, isEmpty);
      expect(tester.widget<PageFrame>(find.byType(PageFrame)).failure, isNull);
      final offline = tester.widget<Slidable>(
        slidable(('first', 'same-session')),
      );
      expect(
        (offline.endActionPane!.children.first as SwipeAction).onPressed,
        isNull,
      );
      session.selectHost(second.id);
      await tester.pump();
      expect(find.text('first-conversation'), findsNothing);
      expect(find.text('second-conversation'), findsOneWidget);
      expect(secondCalls, ['read_conversation']);
      final row = tester.widget<Slidable>(slidable(('second', 'same-session')));
      final archive = row.endActionPane!.children.first as SwipeAction;
      archive.onPressed!(tester.element(find.byType(LiveTasksPage)));
      await tester.pump();
      expect(secondCalls.last, 'set_session_archived');
      expect(firstCalls, isEmpty);
      expect(session.selectedHost, same(second));
      await release(tester, session);
    },
  );

  testWidgets('project selection changes global host and project together', (
    tester,
  ) async {
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
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, 'shared-id');
    expect(session.selectedWorktreeId, 'second-tree');
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
  });

  testWidgets('offline cached tasks and archive follow the selected host', (
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
    tester.widget<TaskFilters>(find.byType(TaskFilters)).onArchive();
    await tester.pump();
    expect(find.text('second-conversation'), findsNothing);
    await tester.tap(find.text('Back'));
    await tester.pump();
    session.selectHost(second.id);
    await tester.pump();
    expect(find.text('first-conversation'), findsNothing);
    tester.widget<TaskFilters>(find.byType(TaskFilters)).onArchive();
    await tester.pump();
    expect(find.text('second-conversation'), findsOneWidget);
    expect(find.text('first-conversation'), findsNothing);
    await release(tester, session);
  });

  testWidgets('all-project and unassigned filters stay on the current host', (
    tester,
  ) async {
    final first = source('first');
    final second = source('second');
    second.snapshot['sessions'] = [
      conversation('second-project-chat'),
      {...conversation('second-unassigned', project: null), 'id': 'unassigned'},
    ];
    final session = AppSession.test(hosts: [first, second]);
    session.selectProject(second.id, 'shared-id');
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    expect(find.text('second-project-chat'), findsOneWidget);
    expect(find.text('second-unassigned'), findsNothing);

    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(
      find.widgetWithText(
        ListTile,
        tester.element(find.byType(LiveTasksPage)).tr('unassignedChats'),
      ),
    );
    await tester.pumpAndSettle();
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, '');
    expect(session.selectedWorktreeId, isNull);
    expect(find.text('second-project-chat'), findsNothing);
    expect(find.text('second-unassigned'), findsOneWidget);
    expect(find.text('first-conversation'), findsNothing);

    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(
      find.widgetWithText(
        ListTile,
        tester.element(find.byType(LiveTasksPage)).tr('allProjects'),
      ),
    );
    await tester.pumpAndSettle();
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, isNull);
    expect(find.text('second-project-chat'), findsOneWidget);
    expect(find.text('second-unassigned'), findsOneWidget);
    expect(find.text('first-conversation'), findsNothing);
    await release(tester, session);
  });

  testWidgets('new conversation captures the globally selected workspace', (
    tester,
  ) async {
    final first = source('first');
    final second = source('second');
    second.snapshot['worktrees'] = [
      {
        'id': 'second-tree',
        'project': 'shared-id',
        'path': '/second',
        'main': true,
      },
      {'id': 'second-linked', 'project': 'shared-id', 'path': '/second-linked'},
    ];
    final session = AppSession.test(hosts: [first, second]);
    session.selectWorkspace(second.id, 'second-linked');
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    await tester.tap(
      find.byWidgetPredicate(
        (widget) => widget is RoundButton && widget.icon == 'plus',
      ),
    );
    await tester.pumpAndSettle();
    final creation = tester.widget<NewConversationPage>(
      find.byType(NewConversationPage),
    );
    expect(creation.host, same(second));
    expect(creation.initialProject, 'shared-id');
    expect(creation.initialWorktree, 'second-linked');
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
    expect(rows.length, 1);
    expect(rows[0].host, same(first));
    expect(find.text('second-terminal'), findsNothing);
    session.selectHost(second.id);
    await tester.pump();
    expect(
      tester.widget<TerminalTask>(find.byType(TerminalTask)).host,
      same(second),
    );
    expect(find.text('first-terminal'), findsNothing);
    final terminal = tester.widget<Slidable>(
      slidable(('terminal', 'second', 'same-terminal')),
    );
    final close = terminal.endActionPane!.children.single as SwipeAction;
    close.onPressed!(tester.element(find.byType(LiveTasksPage)));
    await tester.pump();
    expect(calls.last, 'close_terminal');
    await release(tester, session);
  });

  testWidgets('terminal workspace choice follows and updates global context', (
    tester,
  ) async {
    final first = source('first');
    final second = source('second');
    second.snapshot['worktrees'] = [
      {
        'id': 'second-tree',
        'project': 'shared-id',
        'path': '/second',
        'main': true,
      },
      {'id': 'second-linked', 'project': 'shared-id', 'path': '/second-linked'},
    ];
    final session = AppSession.test(hosts: [first, second]);
    session.selectWorkspace(second.id, 'second-linked');
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pump();
    await tester.tap(
      find.byWidgetPredicate(
        (widget) => widget is RoundButton && widget.icon == 'terminal',
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<ListTile>(find.widgetWithText(ListTile, 'second-linked'))
          .selected,
      isTrue,
    );
    await tester.tap(find.widgetWithText(ListTile, 'second-project'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, 'shared-id');
    expect(session.selectedWorktreeId, 'second-tree');
    final terminal = tester.widget<TerminalPage>(find.byType(TerminalPage));
    expect(terminal.hostId, second.id);
    expect(terminal.worktreeId, 'second-tree');
    expect(terminal.createNew, isTrue);
    await release(tester, session);
  });

  testWidgets(
    'projects follow the host and open their workspace without filtering chats',
    (tester) async {
      final first = source('first', connected: false);
      final second = source('second');
      final session = AppSession.test(hosts: [first, second]);
      await tester.pumpWidget(screen(session, ProjectsPage(host: first)));
      await tester.pump();
      expect(find.text('first-project'), findsOneWidget);
      expect(find.text('second-project'), findsNothing);
      final offline = tester.widget<ListTile>(
        find.ancestor(
          of: find.text('first-project'),
          matching: find.byType(ListTile),
        ),
      );
      expect(offline.enabled, isFalse);
      expect(offline.onTap, isNull);
      expect(find.text('Offline'), findsOneWidget);
      expect(tester.widget<PageFrame>(find.byType(PageFrame)).failure, isNull);
      session.selectHost(second.id);
      await tester.pump();
      expect(find.text('first-project'), findsNothing);
      expect(find.text('second-project'), findsOneWidget);
      await tester.tap(find.text('second-project'));
      await tester.pumpAndSettle();
      final resource = tester.widget<ResourcesPage>(find.byType(ResourcesPage));
      expect(resource.hostId, second.id);
      expect(resource.worktreeId, 'second-tree');
      expect(resource.projectId, 'shared-id');
      expect(session.selectedHost, same(second));
      expect(session.selectedProjectId, isNull);
      expect(session.selectedWorktreeId, isNull);
      await release(tester, session);
    },
  );

  testWidgets('task search can scroll with a compact keyboard viewport', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(640, 360);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetViewInsets);
    final session = AppSession.test(hosts: [source('first')]);
    await tester.pumpWidget(screen(session, LiveTasksPage(session: session)));
    await tester.pumpAndSettle();
    final row = find.text('first-conversation');
    await tester.tap(find.byTooltip(tester.element(row).tr('searchTasks')));
    await tester.pumpAndSettle();
    tester.view.viewInsets = const FakeViewPadding(bottom: 232);
    await tester.pumpAndSettle();

    final top = tester.getTopLeft(row).dy;
    await tester.dragFrom(const Offset(320, 109), const Offset(0, -75));
    await tester.pumpAndSettle();
    expect(
      row.hitTestable().evaluate().isNotEmpty ||
          tester.getTopLeft(row).dy < top,
      isTrue,
    );
    await release(tester, session);
  });
}
