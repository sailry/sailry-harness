import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/project_icon.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

void main() {
  testWidgets('session rows preserve the shared node order', (tester) async {
    for (final order in [
      ['newest', 'middle', 'oldest'],
      ['oldest', 'newest', 'middle'],
    ]) {
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        command: (_, _) async => {},
        snapshot: {
          'sessions': [
            for (final id in order)
              {
                ...fixture.session(id: id),
                'activity': {
                  'title': id,
                  'run': {'status': 'completed'},
                },
              },
          ],
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(tester, const TasksPage(), app);
      await tester.pumpAndSettle();
      final positions = [
        for (final id in order) tester.getTopLeft(find.text(id)).dy,
      ];
      expect(positions[0], lessThan(positions[1]));
      expect(positions[1], lessThan(positions[2]));
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    }
  });

  for (final brightness in Brightness.values) {
    testWidgets('completion attention uses theme color in $brightness', (
      tester,
    ) async {
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        command: (_, _) async => {},
        snapshot: {
          'sessions': [
            for (final state in ['completed', 'cancelled', 'approval'])
              {
                ...fixture.session(id: state),
                'activity': {
                  'title': 'Task $state',
                  'attention': {'revision': 1, 'unread': state == 'completed'},
                  if (state == 'approval') 'waiting': 'approval',
                  'run': {'status': state == 'approval' ? 'running' : state},
                },
              },
          ],
        },
      );
      final app = AppSession.test(hosts: [host]);
      await tester.pumpWidget(
        SessionScope(
          session: app,
          child: MaterialApp(
            theme: SailryTheme.of(brightness),
            home: const TasksPage(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final colors = SailryTheme.of(brightness).colorScheme;
      expect(find.byKey(const ValueKey('task-status-dot')), findsNothing);
      final dot = tester.widget<Container>(
        find.byKey(const ValueKey('task-unread-dot')),
      );
      expect((dot.decoration! as BoxDecoration).color, colors.primary);
      expect(find.byTooltip(tr('approval')), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    });
  }

  testWidgets('list and project picker follow the global host', (tester) async {
    final hosts = [
      for (final id in ['A', 'B'])
        HostConnection.test(
          id: id,
          label: 'Host $id',
          command: (_, _) async => {},
          snapshot: {
            'projects': [
              {
                'id': 'project-$id',
                'name': 'Project $id',
                'appearance': {'icon': 'ai', 'color': 'blue'},
              },
            ],
            'sessions': [
              {
                ...fixture.session(id: 'session-$id'),
                'project': 'project-$id',
                'activity': {
                  'title':
                      'Task $id with a very long title that must not wrap over multiple lines',
                  'run': {'status': 'completed'},
                },
              },
            ],
          },
        ),
    ];
    final app = AppSession.test(hosts: hosts);
    await fixture.mount(tester, const TasksPage(), app);
    await tester.pumpAndSettle();
    expect(find.textContaining('Task A'), findsOneWidget);
    expect(find.textContaining('Task B'), findsNothing);
    expect(find.text('Host A'), findsNothing);
    final title = find.textContaining('Task A');
    expect(tester.widget<Text>(title).maxLines, 1);
    expect(tester.widget<Text>(title).overflow, TextOverflow.ellipsis);
    expect(
      tester.getRect(title).left,
      greaterThan(tester.getRect(find.byType(ProjectIcon)).right),
    );
    expect(find.byType(ProjectIcon), findsOneWidget);
    await tester.tap(find.text(tr('allProjects')));
    await tester.pumpAndSettle();
    expect(find.text('Host A'), findsNothing);
    expect(find.text('Project B'), findsNothing);
    final action = tester.getRect(
      find.widgetWithText(ListTile, tr('hostRegisterProject')),
    );
    final all = tester.getRect(
      find.widgetWithText(ListTile, tr('allProjects')),
    );
    final project = tester.getRect(find.widgetWithText(ListTile, 'Project A'));
    expect(action.bottom, lessThan(all.top));
    expect(find.byType(Divider), findsOneWidget);
    expect(all.height, 48);
    expect(project.top, all.bottom);
    await tester.tap(find.widgetWithText(ListTile, 'Project A'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Host B'));
    await tester.pumpAndSettle();
    expect(app.selectedHost, hosts.last);
    expect(find.textContaining('Task B'), findsOneWidget);
    expect(find.textContaining('Task A'), findsNothing);
    expect(find.text('Host B'), findsNothing);
    expect(find.text(tr('allProjects')), findsOneWidget);
    await tester.tap(find.text(tr('allProjects')));
    await tester.pumpAndSettle();
    expect(find.text('Project A'), findsNothing);
    expect(find.text('Host B'), findsNothing);
    expect(find.text(tr('hostRegisterProject')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('project icons match desktop SVGs', (tester) async {
    final source = File(
      '../../crates/protocol/src/projects.rs',
    ).readAsStringSync();
    final declaration = RegExp(
      r'pub const ICONS: &\[&str\] = &\[(.*?)\];',
      dotAll: true,
    ).firstMatch(source)![1]!;
    final assets = RegExp(
      r'"([^"]+)"',
    ).allMatches(declaration).map((match) => match[1]!).toList();
    expect(assets, hasLength(30));
    expect(ProjectIcon.assets, assets);
    for (final asset in ProjectIcon.assets) {
      expect(
        await rootBundle.loadString('assets/icons/projects/$asset.svg'),
        File('../../assets/icons/reicon/$asset.svg').readAsStringSync(),
      );
    }
  });
}
