import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/features/resources/git_page.dart';
import 'package:sailry_mobile/features/resources/overview_cards.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/features/resources/workspace.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

const scratchName = '00000000-0000-4000-8000-000000000001';
const scratch = {
  'id': 'scratch',
  'project': null,
  'path': '/fixture/workspaces/sessions/$scratchName',
};

class _UpdatingHost extends HostConnection {
  _UpdatingHost() : super.test(id: 'host', label: 'Host');

  void publish(Map<String, dynamic> value) {
    snapshot = value;
    notifyListeners();
  }
}

HostConnection source({CommandHandler? command}) => HostConnection.test(
  id: 'host',
  label: 'Host',
  command: command,
  snapshot: {
    'projects': [
      {'id': 'project', 'name': 'User project', 'path': '/fixture/repository'},
    ],
    'worktrees': [
      scratch,
      {'id': 'orphan', 'project': 'removed', 'path': '/fixture/orphan'},
      {
        'id': 'linked',
        'project': 'project',
        'path': '/fixture/feature',
        'main': false,
      },
      {
        'id': 'main',
        'project': 'project',
        'path': '/fixture/repository',
        'main': true,
      },
    ],
  },
);

Widget screen(AppSession session, Widget child) => SessionScope(
  session: session,
  child: MaterialApp(
    theme: SailryTheme.of(Brightness.light),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: Scaffold(body: child),
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

void main() {
  testWidgets('project resources can open Files and Git on a linked worktree', (
    tester,
  ) async {
    final host = source(
      command: (kind, data) async => {
        'data': {
          if (kind == 'inspect_git') ...{
            'kind': 'repository',
            'branch': 'branch-${data!['worktree']}',
          },
        },
      },
    );
    final session = AppSession.test(hosts: [host]);
    session.selectProject(host.id, unassignedProject);
    await tester.pumpWidget(screen(session, const ProjectsPage()));
    await tester.tap(find.text('User project'));
    await tester.pumpAndSettle();
    expect(session.selectedProjectId, unassignedProject);
    expect(session.selectedWorktreeId, isNull);
    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    expect(find.text(scratchName), findsNothing);
    await tester.tap(find.text('feature'));
    await tester.pumpAndSettle();
    expect(session.selectedProjectId, unassignedProject);
    expect(session.selectedWorktreeId, isNull);
    await tester.tap(
      find.descendant(
        of: find.byType(GitOverviewCard),
        matching: find.text('branch-linked'),
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.widget<GitPage>(find.byType(GitPage)).worktreeId, 'linked');
    Navigator.of(tester.element(find.byType(GitPage))).pop();
    await tester.pumpAndSettle();
    await tester.tap(find.text('Files'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<FilesPage>(find.byType(FilesPage)).worktreeId,
      'linked',
    );
    await release(tester, session);
  });

  testWidgets(
    'general resources follow the selected project while session targets stay explicit',
    (tester) async {
      final host = source();
      final session = AppSession.test(hosts: [host]);
      ResourceTarget? general;
      ResourceTarget? conversation;
      await tester.pumpWidget(
        screen(
          session,
          Builder(
            builder: (context) {
              general = resourceTarget(context, null, null);
              conversation = resourceTarget(context, host.id, 'scratch');
              return const SizedBox.shrink();
            },
          ),
        ),
      );
      expect(general, isNull);
      expect(conversation?.worktree['id'], 'scratch');
      session.selectProject(host.id, 'project');
      await tester.pump();
      expect(general?.worktree['id'], 'main');
      session.selectWorkspace(host.id, 'linked');
      await tester.pump();
      expect(general?.worktree['id'], 'linked');
      session.selectProject(host.id, unassignedProject);
      await tester.pump();
      expect(general, isNull);
      expect(conversation?.worktree['id'], 'scratch');
      expect(projectWorktrees(host).map((tree) => tree['id']), [
        'linked',
        'main',
      ]);
      expect(projectWorktrees(host, project: unassignedProject), isEmpty);
      await release(tester, session);
    },
  );

  testWidgets(
    'workspace picker names registered projects and excludes scratch paths',
    (tester) async {
      final host = source();
      final session = AppSession.test(hosts: [host]);
      String? selected;
      await tester.pumpWidget(
        screen(
          session,
          Builder(
            builder: (context) => TextButton(
              onPressed: () async {
                selected = await showLiveWorkspacePicker(context, host, null);
              },
              child: const Text('Choose'),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Choose'));
      await tester.pumpAndSettle();
      expect(find.text('User project'), findsOneWidget);
      expect(find.text('feature'), findsOneWidget);
      expect(find.text(scratchName), findsNothing);
      expect(find.text(scratch['path']!), findsNothing);
      expect(find.text('orphan'), findsNothing);
      await tester.tap(find.text('User project'));
      await tester.pumpAndSettle();
      expect(selected, 'main');
      await release(tester, session);
    },
  );

  testWidgets(
    'a host with only chat scratch directories has no general workspace',
    (tester) async {
      final host = source()
        ..snapshot = {
          'projects': <dynamic>[],
          'worktrees': [scratch],
        };
      final session = AppSession.test(hosts: [host]);
      ResourceTarget? target;
      await tester.pumpWidget(
        screen(
          session,
          Builder(
            builder: (context) {
              target = resourceTarget(context, null, null);
              return TextButton(
                onPressed: () => showLiveWorkspacePicker(context, host, null),
                child: const Text('Choose'),
              );
            },
          ),
        ),
      );
      expect(target, isNull);
      await tester.tap(find.text('Choose'));
      await tester.pumpAndSettle();
      expect(
        find.text(
          tester.element(find.text('Choose')).tr('resourceNoWorkspace'),
        ),
        findsOneWidget,
      );
      expect(find.text(scratchName), findsNothing);
      await release(tester, session);
    },
  );

  testWidgets(
    'empty resources retain project selection and project registration',
    (tester) async {
      final host = source()
        ..snapshot = {
          'projects': <dynamic>[],
          'worktrees': [scratch],
        };
      final session = AppSession.test(hosts: [host]);
      await tester.pumpWidget(screen(session, const ResourcesPage()));
      expect(find.byType(ProjectBar), findsOneWidget);
      expect(find.text(scratchName), findsNothing);
      await tester.tap(find.byType(ProjectBar));
      await tester.pumpAndSettle();
      expect(find.text('Add project'), findsOneWidget);
      await tester.tap(find.text('Add project'));
      await tester.pumpAndSettle();
      expect(find.byType(ProjectForm), findsOneWidget);
      expect(
        tester.widget<ProjectForm>(find.byType(ProjectForm)).host,
        same(host),
      );
      expect(tester.takeException(), isNull);
      await release(tester, session);
    },
  );

  testWidgets('projects remain visible when their worktrees are unavailable', (
    tester,
  ) async {
    for (final missingTrees in [true, false]) {
      final host = source();
      if (missingTrees) {
        host.snapshot['worktrees'] = <dynamic>[];
      } else {
        host.connected = false;
      }
      final session = AppSession.test(hosts: [host]);
      await tester.pumpWidget(
        screen(
          session,
          Builder(
            builder: (context) => TextButton(
              onPressed: () => showLiveWorkspacePicker(context, host, null),
              child: const Text('Choose'),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Choose'));
      await tester.pumpAndSettle();
      final row = tester.widget<ListTile>(
        find.ancestor(
          of: find.text('User project'),
          matching: find.byType(ListTile),
        ),
      );
      expect(row.enabled, isFalse);
      expect(row.onTap, isNull);
      expect(find.text(scratchName), findsNothing);
      await release(tester, session);
    }
  });

  testWidgets('an empty entry can select a workspace from its routed host', (
    tester,
  ) async {
    final entry = HostConnection.test(id: 'entry', label: 'Entry');
    final routed = HostConnection.test(
      id: 'entry/worker',
      label: 'Worker',
      metadata: {
        'resource_source': {
          'revision': 7,
          'projects': [
            {
              'id': 'project',
              'name': 'Worker project',
              'path': '/fixture/repo',
            },
          ],
          'worktrees': [
            {
              'id': 'main',
              'project': 'project',
              'path': '/fixture/repo',
              'main': true,
            },
          ],
        },
      },
      command: (_, _) async => {'data': <String, dynamic>{}},
    );
    final session = AppSession.test(hosts: [entry, routed]);
    await tester.pumpWidget(screen(session, const ResourcesPage()));
    final selectHost = tester.element(find.byType(ProjectBar)).tr('selectHost');
    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(find.text(selectHost));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Worker'));
    await tester.pumpAndSettle();
    expect(session.selectedHost, same(routed));
    expect(session.selectedProjectId, isNull);
    expect(find.text('Worker project'), findsNothing);
    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Worker project'));
    await tester.pumpAndSettle();
    expect(session.selectedProjectId, 'project');
    expect(session.selectedWorktreeId, 'main');
    expect(find.text('Worker project'), findsOneWidget);
    expect(find.text(scratchName), findsNothing);
    expect(tester.takeException(), isNull);
    await release(tester, session);
  });

  testWidgets(
    'resource selection updates global context and clearing it restores the picker',
    (tester) async {
      final host = source();
      final session = AppSession.test(hosts: [host]);
      await tester.pumpWidget(screen(session, const ResourcesPage()));
      expect(session.selectedProjectId, isNull);
      expect(find.text('User project'), findsNothing);
      await tester.tap(find.byType(ProjectBar));
      await tester.pumpAndSettle();
      await tester.tap(find.text('feature'));
      await tester.pumpAndSettle();
      expect(session.selectedHost, same(host));
      expect(session.selectedProjectId, 'project');
      expect(session.selectedWorktreeId, 'linked');
      expect(find.text('feature'), findsOneWidget);

      session.selectProject(host.id, null);
      await tester.pump();
      expect(session.selectedWorktreeId, isNull);
      expect(find.text('feature'), findsNothing);
      expect(
        tester.widget<ProjectBar>(find.byType(ProjectBar)).onTap,
        isNotNull,
      );
      await release(tester, session);
    },
  );

  testWidgets(
    'explicit resource pages keep their owner after global host changes',
    (tester) async {
      final host = source();
      final other = HostConnection.test(
        id: 'other',
        label: 'Other',
        snapshot: {
          'projects': [
            {'id': 'other-project', 'name': 'Other project'},
          ],
          'worktrees': [
            {
              'id': 'other-tree',
              'project': 'other-project',
              'path': '/other',
              'main': true,
            },
          ],
        },
      );
      final session = AppSession.test(hosts: [host, other]);
      await tester.pumpWidget(
        screen(session, ResourcesPage(hostId: host.id, worktreeId: 'main')),
      );
      await tester.pump();
      session.selectWorkspace(other.id, 'other-tree');
      await tester.pump();
      expect(session.selectedHost, same(other));
      expect(session.selectedProjectId, 'other-project');
      expect(find.text('User project'), findsOneWidget);
      expect(find.text('Other project'), findsNothing);
      expect(tester.widget<ProjectBar>(find.byType(ProjectBar)).onTap, isNull);
      expect(tester.takeException(), isNull);
      await release(tester, session);
    },
  );

  testWidgets('an open workspace picker follows later host snapshots', (
    tester,
  ) async {
    final host = _UpdatingHost();
    final session = AppSession.test(hosts: [host]);
    String? selected;
    await tester.pumpWidget(
      screen(
        session,
        Builder(
          builder: (context) => TextButton(
            onPressed: () async {
              selected = await showLiveWorkspacePicker(context, host, null);
            },
            child: const Text('Choose'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Choose'));
    await tester.pumpAndSettle();
    final emptyMessage = tester
        .element(find.text('Choose'))
        .tr('resourceNoWorkspace');
    expect(find.text(emptyMessage), findsOneWidget);
    host.publish({
      'projects': [
        {
          'id': 'project',
          'name': 'Loaded project',
          'path': '/fixture/repository',
        },
      ],
      'worktrees': [
        {
          'id': 'main',
          'project': 'project',
          'path': '/fixture/repository',
          'main': true,
        },
      ],
    });
    await tester.pump();
    expect(find.text(emptyMessage), findsNothing);
    expect(find.text('Loaded project'), findsOneWidget);
    await tester.tap(find.text('Loaded project'));
    await tester.pumpAndSettle();
    expect(selected, 'main');
    await release(tester, session);
  });
}
