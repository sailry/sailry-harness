import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/lean.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/resources/contributions.dart';
import 'package:sailry_mobile/features/resources/ports_contribution.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  testWidgets(
    'lean project resources show ports without Code resources or reads',
    (tester) async {
      final calls = <String>[];
      final host = HostConnection.test(
        id: 'entry/fixture',
        label: 'Fixture host',
        command: (kind, _) async {
          calls.add(kind);
          return {'data': {}};
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
        },
      );
      final session = AppSession.test(hosts: [host]);
      await tester.pumpWidget(
        SailryApp(assembly: leanAssembly, session: session),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('tab-$projectsRoute')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Fixture project'));
      await tester.pumpAndSettle();

      final resources = tester.element(find.byType(ResourcesPage));
      expect(ProjectResourcesScope.of(resources), [portsResource]);
      expect(find.text(resources.tr('files')), findsNothing);
      expect(find.text(resources.tr('activityPorts')), findsOneWidget);
      expect(calls, isNot(contains('inspect_git')));
      expect(calls, isNot(contains('list_git_branches')));
      expect(calls, isNot(contains('list_directory')));

      await tester.pumpWidget(const SizedBox.shrink());
      await session.close();
      session.dispose();
    },
  );
}
