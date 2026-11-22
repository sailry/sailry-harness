import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/terminal.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/harness.dart';
import 'package:sailry_mobile/assembly/lean.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/conversations/live/references.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/file_page.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/features/resources/git/overview_card.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:shared_preferences/shared_preferences.dart';

const _routeKey = 'navigation.route';
const _project = 'Fixture project';
const _worktree = 'tree';
const _terminal = {
  'id': 'shell',
  'worktree': _worktree,
  'access': 'unrestricted',
  'status': {'kind': 'running'},
  'revision': 1,
};

class _Preferences extends Fake implements SharedPreferencesAsync {
  _Preferences({this.pendingRoute});

  final values = <String, Object>{
    'language': 'english',
    'notifications.in_app': false,
  };
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

class _TerminalObservation extends Fake implements TerminalUpdates {
  final result = Completer<String>();
  int closes = 0;
  int releases = 0;

  @override
  Future<String> next() => result.future;

  @override
  Future<void> close() async {
    closes++;
    result.complete('{}');
  }

  @override
  void dispose() => releases++;
}

class _TerminalConnection extends Fake implements Connection {
  final updates = _TerminalObservation();
  final watched = <String>[];

  @override
  Future<TerminalUpdates> watchTerminal({required String terminal}) async {
    watched.add(terminal);
    return updates;
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

HostConnection _host({required CommandHandler command, Connection? bridge}) =>
    HostConnection.test(
      id: 'entry/fixture',
      label: 'Fixture host',
      connection: bridge,
      command: command,
      snapshot: const {
        'projects': [
          {'id': 'project', 'name': _project, 'path': '/fixture'},
        ],
        'worktrees': [
          {
            'id': _worktree,
            'project': 'project',
            'main': true,
            'path': '/fixture',
          },
        ],
        'terminals': [_terminal],
      },
    );

Future<void> _mount(
  WidgetTester tester,
  AppSession session,
  MobileAssembly assembly,
  _Preferences preferences,
) async {
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
    await _mount(tester, session, harnessAssembly, preferences);
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
      pages: harnessAssembly.pages.reversed,
      defaultRoute: chatRoute,
    );
    await _mount(tester, session, reordered, preferences);
    expect(_navigation(tester).items.first.id, settingsRoute);
    expect(_navigation(tester).selected, settingsRoute);

    final withoutSettings = MobileAssembly(
      pages: harnessAssembly.pages.where((page) => page.id != settingsRoute),
      defaultRoute: chatRoute,
    );
    await _mount(tester, session, withoutSettings, preferences);
    expect(_tab(settingsRoute), findsNothing);
    expect(_navigation(tester).selected, chatRoute);
    expect(preferences.values[_routeKey], settingsRoute);

    await _mount(tester, session, harnessAssembly, preferences);
    expect(_navigation(tester).selected, settingsRoute);
    await _release(tester, session);
  });

  testWidgets('a tab selected during startup wins over a late saved route', (
    tester,
  ) async {
    final session = AppSession.test();
    final saved = Completer<String?>();
    final preferences = _Preferences(pendingRoute: saved.future);
    await _mount(tester, session, harnessAssembly, preferences);
    await tester.tap(_tab(projectsRoute));
    await tester.pumpAndSettle();
    saved.complete(settingsRoute);
    await tester.pumpAndSettle();
    expect(_navigation(tester).selected, projectsRoute);
    expect(preferences.values[_routeKey], projectsRoute);
    await _release(tester, session);
  });

  testWidgets('Host counts use the selected task and project destinations', (
    tester,
  ) async {
    final host = _host(
      command: (_, _) async => {
        'data': {'network': {}},
      },
    );
    final session = AppSession.test(hosts: [host]);
    final preferences = _Preferences()..values[_routeKey] = hostsRoute;
    final reordered = MobileAssembly(
      pages: [
        for (final page in harnessAssembly.pages.reversed)
          if (page.id == projectsRoute)
            MobilePage(
              id: page.id,
              icon: page.icon,
              label: page.label,
              build: (context) {
                final owner = context.arguments as HostConnection?;
                return Text(
                  owner == null ? 'All projects' : 'Projects on ${owner.label}',
                );
              },
            )
          else
            page,
      ],
      defaultRoute: chatRoute,
    );
    await _mount(tester, session, reordered, preferences);
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
      pages: harnessAssembly.pages.where((page) => page.id == hostsRoute),
      defaultRoute: hostsRoute,
    );
    await _mount(tester, session, hostsOnly, preferences);
    expect(tester.widget<InkWell>(sessions).onTap, isNull);
    expect(tester.widget<InkWell>(projects).onTap, isNull);
    expect(find.byType(TasksPage, skipOffstage: false), findsNothing);
    expect(find.byType(ProjectsPage, skipOffstage: false), findsNothing);
    await _release(tester, session);
  });

  testWidgets(
    'Git work is scoped to its contribution and stops updating on pop',
    (tester) async {
      final calls = <String>[];
      final inspect = Completer<Map<String, dynamic>>();
      final host = _host(
        command: (kind, data) async {
          calls.add(kind);
          if (kind == 'inspect_git') {
            expect(data?['worktree'], _worktree);
            return inspect.future;
          }
          return {'data': {}};
        },
      );
      final session = AppSession.test(hosts: [host]);
      final preferences = _Preferences()..values[_routeKey] = projectsRoute;
      await _mount(tester, session, harnessAssembly, preferences);
      await tester.tap(find.text(_project));
      await tester.pumpAndSettle();
      expect(find.text('Files'), findsOneWidget);
      expect(find.byType(GitOverviewCard), findsOneWidget);
      expect(calls.where((kind) => kind == 'inspect_git'), hasLength(1));

      Navigator.of(tester.element(find.byType(ResourcesPage))).pop();
      await tester.pumpAndSettle();
      inspect.complete({
        'data': {'kind': 'repository', 'branch': 'main'},
      });
      await tester.pumpAndSettle();
      expect(calls, isNot(contains('list_git_branches')));

      calls.clear();
      await _mount(tester, session, leanAssembly, preferences);
      await tester.tap(find.text(_project));
      await tester.pumpAndSettle();
      expect(find.text('Files'), findsNothing);
      expect(find.byType(GitOverviewCard, skipOffstage: false), findsNothing);
      final resources = tester.element(find.byType(ResourcesPage));
      expect(find.text(resources.tr('activityPorts')), findsOneWidget);
      expect(calls, isNot(contains('inspect_git')));
      expect(calls, isNot(contains('list_git_branches')));
      expect(calls, isNot(contains('list_directory')));
      await _release(tester, session);
    },
  );

  testWidgets(
    'directory references use Files when installed and file previews remain in lean',
    (tester) async {
      final calls = <(String, Map<String, dynamic>?)>[];
      final host = _host(
        command: (kind, data) async {
          calls.add((kind, data));
          return {
            'data': switch (kind) {
              'read_file' => {'text': 'fixture contents', 'revision': 'v1'},
              'list_directory' => {'entries': []},
              _ => {},
            },
          };
        },
      );
      final session = AppSession.test(hosts: [host]);
      final preferences = _Preferences();
      MobileAssembly references(MobileAssembly base) => MobileAssembly(
        defaultRoute: chatRoute,
        resources: base.resources,
        pages: [
          MobilePage(
            id: chatRoute,
            icon: 'chat',
            label: 'chat',
            build: (_) => Center(
              child: MessageReferences(
                host: host,
                worktree: _worktree,
                references: const [
                  {
                    'label': 'Source directory',
                    'target': {'kind': 'directory', 'data': 'src'},
                  },
                  {
                    'label': 'Source file',
                    'target': {'kind': 'file', 'data': 'src/lib.txt'},
                  },
                ],
              ),
            ),
          ),
        ],
      );

      await _mount(tester, session, references(harnessAssembly), preferences);
      await tester.tap(find.text('Source directory'));
      await tester.pumpAndSettle();
      final page = tester.widget<FilesPage>(find.byType(FilesPage));
      expect(
        (page.hostId, page.worktreeId, page.initialDirectory),
        (host.id, _worktree, 'src'),
      );
      expect(calls.singleWhere((call) => call.$1 == 'list_directory').$2, {
        'worktree': _worktree,
        'path': 'src',
        'after': null,
      });

      calls.clear();
      await _mount(tester, session, references(leanAssembly), preferences);
      await tester.tap(find.text('Source directory'));
      await tester.pumpAndSettle();
      final context = tester.element(find.byType(MessageReferences));
      expect(find.text(context.tr('fileLinkUnavailable')), findsOneWidget);
      expect(find.byType(FilesPage), findsNothing);
      expect(calls.map((call) => call.$1), isNot(contains('list_directory')));
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();

      await tester.tap(find.text('Source file'));
      await tester.pumpAndSettle();
      final file = tester.widget<ResourceFilePage>(
        find.byType(ResourceFilePage),
      );
      expect(
        (file.host, file.worktree, file.path),
        (host, _worktree, 'src/lib.txt'),
      );
      expect(calls.singleWhere((call) => call.$1 == 'read_file').$2, {
        'worktree': _worktree,
        'path': 'src/lib.txt',
      });
      expect(file.document.draft?.text, 'fixture contents');
      await _release(tester, session);
    },
  );

  testWidgets(
    'closing a lean project terminal releases its observation and leaves the shell running',
    (tester) async {
      final connection = _TerminalConnection();
      final calls = <String>[];
      final host = _host(
        bridge: connection,
        command: (kind, data) async {
          calls.add(kind);
          if (kind == 'open_terminal') {
            expect(data?['terminal'], 'shell');
            return {'data': _terminal};
          }
          return {'data': {}};
        },
      );
      final session = AppSession.test(hosts: [host]);
      final preferences = _Preferences()..values[_routeKey] = projectsRoute;
      await _mount(tester, session, leanAssembly, preferences);
      await tester.tap(find.text(_project));
      await tester.pumpAndSettle();
      final context = tester.element(find.byType(ResourcesPage));
      await tester.tap(find.text(context.tr('terminal')));
      await tester.pumpAndSettle();
      expect(connection.watched, ['shell']);
      expect(calls, contains('open_terminal'));
      expect(connection.updates.closes, 0);

      Navigator.of(tester.element(find.byType(TerminalPage))).pop();
      await tester.pumpAndSettle();
      expect(connection.updates.closes, 1);
      expect(connection.updates.releases, 1);
      expect(calls, isNot(contains('close_terminal')));
      expect(host.snapshot['terminals'], [_terminal]);
      await _release(tester, session);
    },
  );
}
