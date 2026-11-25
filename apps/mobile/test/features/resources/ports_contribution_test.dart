import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/harness.dart';
import 'package:sailry_mobile/features/resources/ports_contribution.dart';
import 'package:sailry_mobile/features/resources/ports_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/page_heading.dart';

void main() {
  testWidgets('Ports opens the same host from the host and project pages', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'host',
      label: 'Fixture host',
      command: (_, _) async => {
        'data': {'network': <String, dynamic>{}},
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
    expect(portsResource.hostCount?.id, 'ports');
    expect(portsResource.taskSection, isNull);
    expect(portsResource.hostCount!.changes!(host), same(host.ports));
    expect(portsResource.hostCount!.count(host), 0);
    await tester.pumpWidget(
      SailryApp(assembly: harnessAssembly, session: session),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-hosts')));
    await tester.pumpAndSettle();
    final portsCount = find.byKey(const ValueKey('host-ports'));
    expect(portsCount, findsOneWidget);
    await tester.tap(portsCount);
    await tester.pumpAndSettle();
    final portsPage = find.byType(PortsPage);
    expect(tester.widget<PortsPage>(portsPage).host, same(host));
    final back = find.descendant(
      of: find.byType(PageHeading),
      matching: find.byTooltip(tester.element(portsPage).tr('back')),
    );
    expect(back, findsOneWidget);
    await tester.tap(back);
    await tester.pumpAndSettle();
    expect(portsPage, findsNothing);

    await tester.tap(find.byKey(const ValueKey('tab-projects')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Fixture project'));
    await tester.pumpAndSettle();
    final resources = find.byType(ResourcesPage);
    await tester.tap(
      find.descendant(
        of: resources,
        matching: find.text(tester.element(resources).tr('activityPorts')),
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.widget<PortsPage>(portsPage).host, same(host));
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });
}
