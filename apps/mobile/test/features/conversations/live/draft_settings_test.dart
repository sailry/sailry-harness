import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/harness.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/conversations/live/draft_settings.dart';
import 'package:sailry_mobile/features/conversations/live/team.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';

void main() {
  testWidgets('Harness draft keeps project and model UI without branch reads', (
    tester,
  ) async {
    final calls = <String>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Fixture host',
      command: (kind, _) async {
        calls.add(kind);
        return {'data': <String, dynamic>{}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    final team = ChangeNotifier();
    final assembly = MobileAssembly(
      pages: [
        MobilePage(
          id: chatRoute,
          icon: 'chat',
          label: 'chat',
          build: (_) => DraftSettings(
            host: host,
            projects: const [
              {'id': 'project', 'name': 'Fixture project'},
            ],
            worktrees: const [
              {'id': 'tree', 'project': 'project', 'main': true},
            ],
            project: const {'id': 'project', 'name': 'Fixture project'},
            worktree: const {'id': 'tree', 'project': 'project', 'main': true},
            providers: const [],
            config: const {},
            team: TeamChoices(
              listenable: team,
              read: () => const SessionTeam({}, []),
              onTeam: (_) async {},
              onMember: (_, _) async {},
            ),
            onProject: (_) {},
            onWorktree: (_) => fail('Harness offered a branch choice'),
            onConfig: (_) {},
          ),
        ),
      ],
      defaultRoute: chatRoute,
      resources: harnessAssembly.resources,
      draftWorkspace: harnessAssembly.draftWorkspace,
    );
    await tester.pumpWidget(SailryApp(assembly: assembly, session: session));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('draft-project')), findsOneWidget);
    expect(find.byKey(const ValueKey('draft-worktree')), findsNothing);
    expect(find.byKey(const ValueKey('configuration-mode')), findsNothing);
    expect(
      find.byKey(const ValueKey('configuration-permission')),
      findsOneWidget,
    );
    expect(calls, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    team.dispose();
  });
}
