import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:path_provider_platform_interface/path_provider_platform_interface.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/harness.dart';
import 'package:sailry_mobile/assembly/lean.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/contributions.dart';
import 'package:sailry_mobile/features/resources/ports_contribution.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/features/terminal/project_resource.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/features/updates/presentation.dart';
import 'package:sailry_mobile/features/updates/service.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';
import 'package:sailry_mobile/shell/shell.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:shared_preferences_platform_interface/in_memory_shared_preferences_async.dart';
import 'package:shared_preferences_platform_interface/shared_preferences_async_platform_interface.dart';

HostConnection _fixtureHost(List<String> calls) => HostConnection.test(
  id: 'host',
  label: 'Fixture host',
  command: (kind, _) async {
    calls.add(kind);
    return {
      'data': {'network': <String, dynamic>{}},
    };
  },
  snapshot: {
    'projects': [
      {'id': 'project', 'name': 'Fixture project', 'path': '/fixture'},
    ],
    'sessions': [
      {
        'id': 'chat',
        'project': 'project',
        'revision': 1,
        'activity': {'title': 'Fixture chat'},
      },
    ],
    'worktrees': [
      {'id': 'tree', 'project': 'project', 'main': true, 'path': '/fixture'},
    ],
    'terminals': [
      {
        'id': 'shell',
        'title': 'Fixture shell',
        'worktree': 'tree',
        'status': {'kind': 'running'},
      },
    ],
  },
);

List<String> _countCells(WidgetTester tester) => [
  for (final cell in tester.widgetList<InkWell>(
    find.byWidgetPredicate(
      (widget) =>
          widget is InkWell &&
          widget.key is ValueKey<String> &&
          (widget.key as ValueKey<String>).value.startsWith('host-'),
    ),
  ))
    (cell.key as ValueKey<String>).value,
];

class _StartupPath extends PathProviderPlatform {
  final path = Completer<String>();

  @override
  Future<String?> getApplicationSupportPath() => path.future;
}

void main() {
  test('resource link registration rejects duplicate routes', () {
    Future<void> open(BuildContext _, ProjectResourceLink _) async {}
    Future<void> legacyOpen(
      BuildContext _,
      HostConnection _,
      String _,
      String _,
    ) async {}

    final directory = ProjectLinkContribution.target('directory', open);
    final explicit = ProjectResourceFeature(links: [directory]);
    final legacy = ProjectResourceFeature(openDirectory: legacyOpen);
    final both = ProjectResourceFeature(
      openDirectory: legacyOpen,
      links: [directory],
    );
    const scope = (host: 'entry/target', worktree: 'tree', session: 'session');
    final duplicateSchemes = ProjectResourceFeature(
      links: [
        ProjectLinkContribution.scheme(
          'https',
          open,
          scope: scope,
          priority: 3,
        ),
        ProjectLinkContribution.scheme(
          'HTTPS',
          open,
          scope: scope,
          priority: 3,
        ),
      ],
    );
    final systemTie = ProjectResourceFeature(
      links: [ProjectLinkContribution.scheme('https', open, priority: -1)],
    );
    final scopedSystemTie = ProjectResourceFeature(
      links: [
        ProjectLinkContribution.scheme(
          'https',
          open,
          scope: scope,
          priority: -1,
        ),
      ],
    );
    final global = ProjectResourceFeature(
      links: [ProjectLinkContribution.scheme('sample', open, priority: 2)],
    );
    final scoped = ProjectResourceFeature(
      links: [
        ProjectLinkContribution.scheme(
          'sample',
          open,
          scope: scope,
          priority: 2,
        ),
      ],
    );
    for (final (features, route, priority) in [
      ([global, scoped], 'scheme/sample', 2),
      ([scoped, global], 'scheme/sample', 2),
      ([explicit, explicit], 'target/directory', 0),
      ([legacy, legacy], 'target/directory', 0),
      ([legacy, explicit], 'target/directory', 0),
      ([both], 'target/directory', 0),
      ([duplicateSchemes], 'scheme/https', 3),
      ([systemTie], 'scheme/https', -1),
      ([scopedSystemTie], 'scheme/https', -1),
    ]) {
      expect(
        () => MobileAssembly(
          pages: [projectsPage],
          defaultRoute: projectsRoute,
          resources: features,
        ),
        throwsA(
          isA<ArgumentError>().having(
            (error) => error.message,
            'message',
            'Duplicate project link route $route at priority $priority',
          ),
        ),
      );
    }
  });

  test('default pages are exported for independent selection', () {
    expect(harnessPages, [tasksPage, hostsPage, projectsPage, settingsPage]);
    expect(terminalResource.hostCount?.id, 'terminals');
    expect(terminalResource.taskSection?.scope, terminalTaskScope);
  });

  testWidgets(
    'a terminal contribution adds its host count and task section to the default pages',
    (tester) async {
      final calls = <String>[];
      final session = AppSession.test(hosts: [_fixtureHost(calls)]);
      final assembly = MobileAssembly(
        pages: harnessPages,
        defaultRoute: hostsRoute,
        resources: const [terminalResource, portsResource],
      );
      await tester.pumpWidget(SailryApp(assembly: assembly, session: session));
      await tester.pumpAndSettle();
      expect(_countCells(tester), [
        'host-projects',
        'host-sessions',
        'host-terminals',
        'host-ports',
      ]);

      // Running terminal rows animate their status, so the task tab never settles.
      Future<void> show() async {
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 400));
      }

      await tester.tap(find.byKey(const ValueKey('host-terminals')));
      await show();
      final terminal = find.byKey(const ValueKey('terminal-task-shell'));
      expect(terminal, findsOneWidget);
      expect(find.text('Fixture chat'), findsNothing);
      expect(find.byKey(const ValueKey('task-scope-close')), findsOneWidget);
      await tester.tap(find.byKey(const ValueKey('task-scope-close')));
      await show();
      expect(find.text('Fixture chat'), findsOneWidget);
      expect(terminal, findsOneWidget);

      await tester.tap(find.byKey(const ValueKey('tab-hosts')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('host-sessions')));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TasksPage>(find.byType(TasksPage)).sessionsOnly,
        isTrue,
      );
      expect(find.text('Fixture chat'), findsOneWidget);
      expect(terminal, findsNothing);

      await tester.tap(find.byKey(const ValueKey('tab-chat')));
      await show();
      expect(
        tester.widget<TasksPage>(find.byType(TasksPage)).sessionsOnly,
        isFalse,
      );
      expect(terminal, findsOneWidget);
      expect(calls.where((kind) => kind.contains('terminal')), isEmpty);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await session.close();
      session.dispose();
    },
  );

  testWidgets('task sections and host counts need no core branch', (
    tester,
  ) async {
    final calls = <String>[];
    final host = _fixtureHost(calls);
    final session = AppSession.test(hosts: [host]);
    final browsed = <TaskBrowse>[];
    final assembly = MobileAssembly(
      pages: harnessPages,
      defaultRoute: hostsRoute,
      resources: [
        ProjectResourceFeature(
          hostCount: HostResourceCount(
            id: 'jobs',
            label: 'running',
            count: (_) => 7,
            open: (_, view) => switch (view.browseTasks) {
              final browse? => () {
                browsed.add(const TaskBrowse.filter('running'));
                browse(const TaskBrowse.filter('running'));
              },
              null => null,
            },
          ),
          taskSection: TaskListSection(
            scope: 'running',
            records: (_) => [
              {'id': 'job', 'title': 'Fixture job'},
            ],
            include: (_, record, query) =>
                text(record['title']).toLowerCase().contains(query.search),
            row: (_, key, _, record, {required divider}) =>
                Text(text(record['title']), key: key),
          ),
        ),
      ],
    );
    await tester.pumpWidget(SailryApp(assembly: assembly, session: session));
    await tester.pumpAndSettle();
    expect(_countCells(tester), [
      'host-projects',
      'host-sessions',
      'host-jobs',
    ]);
    expect(find.text('7'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('host-jobs')));
    await tester.pumpAndSettle();
    expect(browsed, hasLength(1));
    expect(
      tester.widget<TasksPage>(find.byType(TasksPage)).initialFilter,
      'running',
    );
    expect(find.text('Fixture job'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });

  test('Harness keeps the normal routes and only common project resources', () {
    expect(harnessAssembly.pages.map((page) => page.id), [
      chatRoute,
      hostsRoute,
      projectsRoute,
      settingsRoute,
    ]);
    expect(harnessAssembly.resources, [portsResource]);
    expect(leanAssembly.resources, [portsResource]);
    expect(harnessAssembly.draftWorkspace, isNull);
    expect(harnessAssembly.conversationConfiguration, isNull);
    expect(harnessAssembly.resolveRoute('removed-route'), chatRoute);
  });

  testWidgets('mobile assemblies opt in to app updates', (tester) async {
    final previous = SharedPreferencesAsyncPlatform.instance;
    SharedPreferencesAsyncPlatform.instance =
        InMemorySharedPreferencesAsync.empty();
    addTearDown(() => SharedPreferencesAsyncPlatform.instance = previous);

    // Pause startup before it opens a profile or initializes native transport.
    final previousPaths = PathProviderPlatform.instance;
    final bootstrap = _StartupPath();
    PathProviderPlatform.instance = bootstrap;
    addTearDown(() => PathProviderPlatform.instance = previousPaths);

    try {
      for (final assembly in [harnessAssembly, leanAssembly]) {
        await tester.pumpWidget(
          SailryApp(key: ObjectKey(assembly), assembly: assembly),
        );
        expect(
          tester.widget<MobileShell>(find.byType(MobileShell)).updates,
          isNull,
        );
        expect(find.byType(UpdateDelivery), findsNothing);
      }

      var created = 0;
      final product = MobileAssembly(
        pages: harnessPages,
        defaultRoute: chatRoute,
        createUpdates: () {
          created++;
          return AppUpdates(
            repository: 'sailry/sailry-code',
            tagPrefix: 'mobile-v',
            requireBuildNumber: true,
          );
        },
      );
      await tester.pumpWidget(
        SailryApp(key: ObjectKey(product), assembly: product),
      );
      expect(created, 1);
      final delivery = tester.widget<UpdateDelivery>(
        find.byType(UpdateDelivery),
      );
      expect(
        tester.widget<MobileShell>(find.byType(MobileShell)).updates,
        same(delivery.updates),
      );
    } finally {
      await tester.pumpWidget(const SizedBox.shrink());
      bootstrap.path.complete('/unused');
      await tester.pump();
    }
  });

  testWidgets(
    'Harness has no interactive terminal navigation even with old records',
    (tester) async {
      final calls = <String>[];
      final host = HostConnection.test(
        id: 'host',
        label: 'Fixture host',
        command: (kind, _) async {
          calls.add(kind);
          return {
            'data': {'network': <String, dynamic>{}},
          };
        },
        snapshot: const {
          'projects': [
            {'id': 'project', 'name': 'Fixture project', 'path': '/fixture'},
          ],
          'worktrees': [
            {
              'id': 'tree',
              'project': 'project',
              'main': true,
              'path': '/fixture',
            },
          ],
          'terminals': [
            {
              'id': 'old-shell',
              'worktree': 'tree',
              'status': {'kind': 'running'},
            },
          ],
        },
      );
      final session = AppSession.test(hosts: [host]);
      await tester.pumpWidget(
        SailryApp(assembly: harnessAssembly, session: session),
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('terminal-task-old-shell')),
        findsNothing,
      );
      await tester.tap(find.byKey(const ValueKey('tab-hosts')));
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('host-terminals')), findsNothing);
      expect(_countCells(tester), [
        'host-projects',
        'host-sessions',
        'host-ports',
      ]);
      await tester.tap(find.byKey(const ValueKey('tab-projects')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Fixture project'));
      await tester.pumpAndSettle();
      expect(find.byType(ResourcesPage), findsOneWidget);
      expect(
        find.byWidgetPredicate(
          (widget) => widget is RoundButton && widget.icon == 'terminal',
        ),
        findsNothing,
      );
      expect(find.byType(TerminalPage), findsNothing);
      expect(calls.where((kind) => kind.contains('terminal')), isEmpty);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox.shrink());
      await session.close();
      session.dispose();
    },
  );
}
