import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/workspace.dart';
import 'package:sailry_mobile/features/resources/scope_picker.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'workspace_fixture.dart';

class _UpdatingHost extends HostConnection {
  _UpdatingHost() : super.test(id: 'host', label: 'Host');

  void publish(Map<String, dynamic> value) {
    snapshot = value;
    notifyListeners();
  }
}

void main() {
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
      final row = tester.widget<ScopeRow>(
        find.ancestor(
          of: find.text('User project'),
          matching: find.byType(ScopeRow),
        ),
      );
      expect(row.enabled, isFalse);
      expect(row.onTap, isNull);
      expect(find.text(scratchName), findsNothing);
      await release(tester, session);
    }
  });
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
