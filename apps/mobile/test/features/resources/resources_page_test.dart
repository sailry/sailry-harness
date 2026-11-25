import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'workspace_fixture.dart';

void main() {
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
}
