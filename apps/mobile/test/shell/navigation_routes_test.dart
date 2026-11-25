import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/terminal/project_resource.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _routeKey = 'navigation.route';

final _assembly = MobileAssembly(pages: harnessPages, defaultRoute: chatRoute);

class _Preferences extends Fake implements SharedPreferencesAsync {
  _Preferences({this.pendingRoute});

  final values = <String, Object>{'notifications.in_app': false};
  final Future<String?>? pendingRoute;

  @override
  Future<String?> getString(String key) async =>
      key == _routeKey && pendingRoute != null
      ? await pendingRoute
      : values[key] as String?;

  @override
  Future<bool?> getBool(String key) async => values[key] as bool?;

  @override
  Future<void> setString(String key, String value) async {
    values[key] = value;
  }
}

Future<void> _mount(
  WidgetTester tester,
  AppSession session,
  MobileAssembly assembly, {
  _Preferences? preferences,
}) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await tester.pumpWidget(
    SailryApp(assembly: assembly, preferences: preferences, session: session),
  );
  await tester.pumpAndSettle();
}

Future<void> _release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

Finder _tab(String id) => find.byKey(ValueKey('tab-$id'));

FloatingNavigation _navigation(WidgetTester tester) =>
    tester.widget(find.byType(FloatingNavigation));

void main() {
  testWidgets('root routes survive reorder and recover after omission', (
    tester,
  ) async {
    final session = AppSession.test();
    final preferences = _Preferences();
    await _mount(tester, session, _assembly, preferences: preferences);
    expect(_navigation(tester).selected, chatRoute);
    expect(_navigation(tester).items.map((item) => item.id), [
      chatRoute,
      hostsRoute,
      projectsRoute,
      settingsRoute,
    ]);

    await tester.tap(_tab(settingsRoute));
    await tester.pumpAndSettle();
    expect(preferences.values[_routeKey], settingsRoute);

    final reordered = MobileAssembly(
      pages: harnessPages.reversed,
      defaultRoute: chatRoute,
    );
    await _mount(tester, session, reordered, preferences: preferences);
    expect(_navigation(tester).items.first.id, settingsRoute);
    expect(_navigation(tester).selected, settingsRoute);

    final withoutSettings = MobileAssembly(
      pages: harnessPages.where((page) => page.id != settingsRoute),
      defaultRoute: chatRoute,
    );
    await _mount(tester, session, withoutSettings, preferences: preferences);
    expect(_tab(settingsRoute), findsNothing);
    expect(_navigation(tester).selected, chatRoute);
    expect(preferences.values[_routeKey], settingsRoute);

    await _mount(tester, session, _assembly, preferences: preferences);
    expect(_navigation(tester).selected, settingsRoute);
    await _release(tester, session);
  });

  testWidgets('a tab selected during startup wins over a late saved route', (
    tester,
  ) async {
    final session = AppSession.test();
    final saved = Completer<String?>();
    final preferences = _Preferences(pendingRoute: saved.future);
    await _mount(tester, session, _assembly, preferences: preferences);
    await tester.tap(_tab(projectsRoute));
    await tester.pumpAndSettle();
    saved.complete(settingsRoute);
    await tester.pumpAndSettle();
    expect(_navigation(tester).selected, projectsRoute);
    expect(preferences.values[_routeKey], projectsRoute);
    await _release(tester, session);
  });

  testWidgets('host counts use the selected task and project destinations', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'entry/fixture',
      label: 'Fixture host',
      command: (_, _) async => {
        'data': {'network': {}},
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
            'id': 'shell',
            'worktree': 'tree',
            'status': {'kind': 'running'},
          },
        ],
      },
    );
    final session = AppSession.test(hosts: [host]);
    final assembly = MobileAssembly(
      pages: [
        MobilePage(
          id: projectsRoute,
          icon: projectsPage.icon,
          label: projectsPage.label,
          build: (context) {
            final owner = context.arguments as HostConnection?;
            return Text(
              owner == null ? 'All projects' : 'Projects on ${owner.label}',
            );
          },
        ),
        hostsPage,
        tasksPage,
      ],
      defaultRoute: hostsRoute,
      resources: const [terminalResource],
    );
    await _mount(tester, session, assembly);
    final sessions = find.byKey(const ValueKey('host-sessions'));
    final projects = find.byKey(const ValueKey('host-projects'));
    await tester.ensureVisible(projects);
    await tester.tap(projects);
    await tester.pumpAndSettle();
    final scopedProjects = find.text('Projects on ${host.label}');
    expect(scopedProjects, findsOneWidget);
    Navigator.of(tester.element(scopedProjects)).pop();
    await tester.pumpAndSettle();

    await tester.ensureVisible(sessions);
    await tester.tap(sessions);
    await tester.pumpAndSettle();
    expect(_navigation(tester).selected, chatRoute);
    expect(
      tester.widget<TasksPage>(find.byType(TasksPage)).sessionsOnly,
      isTrue,
    );
    final runningTerminal = find.byKey(const ValueKey('terminal-task-shell'));
    expect(runningTerminal, findsNothing);

    await tester.tap(_tab(chatRoute));
    await tester.pump();
    expect(
      tester.widget<TasksPage>(find.byType(TasksPage)).sessionsOnly,
      isFalse,
    );
    expect(runningTerminal, findsOneWidget);

    final hostsOnly = MobileAssembly(
      pages: [hostsPage],
      defaultRoute: hostsRoute,
    );
    await _mount(tester, session, hostsOnly);
    expect(tester.widget<InkWell>(sessions).onTap, isNull);
    expect(tester.widget<InkWell>(projects).onTap, isNull);
    expect(find.byType(TasksPage, skipOffstage: false), findsNothing);
    expect(find.byType(ProjectsPage, skipOffstage: false), findsNothing);
    await _release(tester, session);
  });
}
