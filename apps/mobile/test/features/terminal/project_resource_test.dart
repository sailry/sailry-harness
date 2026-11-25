import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/terminal.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/assembly/pages.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/features/terminal/project_resource.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/features/terminal/toolbar.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/shell/assembly.dart';
import 'package:sailry_mobile/ui/kit.dart';

const _project = 'Fixture project';
const _worktree = 'tree';
const _terminal = {
  'id': 'shell',
  'worktree': _worktree,
  'access': 'unrestricted',
  'status': {'kind': 'running'},
  'revision': 1,
};

final _assembly = MobileAssembly(
  pages: [projectsPage],
  defaultRoute: projectsRoute,
  resources: const [terminalResource],
);

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

HostConnection _host(Connection connection, CommandHandler command) =>
    HostConnection.test(
      id: 'entry/fixture',
      label: 'Fixture host',
      connection: connection,
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

Future<BuildContext> _openProject(
  WidgetTester tester,
  AppSession session,
) async {
  await tester.pumpWidget(SailryApp(assembly: _assembly, session: session));
  await tester.pumpAndSettle();
  await tester.tap(find.text(_project));
  await tester.pumpAndSettle();
  return tester.element(find.byType(ResourcesPage));
}

Future<void> _release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

void main() {
  testWidgets(
    'a Node without terminals refuses both resource entry points without opening a shell',
    (tester) async {
      final calls = <String>[];
      final connection = _TerminalConnection();
      final host = _host(connection, (kind, _) async {
        calls.add(kind);
        if (kind == 'read_terminal_settings') {
          throw const CommandFailure(
            'not_configured',
            detail: 'terminal module is unavailable',
          );
        }
        return {'data': <String, dynamic>{}};
      });
      final session = AppSession.test(hosts: [host]);
      final context = await _openProject(tester, session);
      final unavailable = context.tr('failureNotConfigured');
      await tester.tap(find.text(context.tr('terminal')));
      await tester.pumpAndSettle();
      expect(find.text(unavailable), findsOneWidget);
      expect(find.byType(TerminalPage), findsOneWidget);
      expect(find.byType(TerminalToolbar), findsNothing);
      expect(find.byKey(const ValueKey('terminal-end')), findsNothing);

      Navigator.of(tester.element(find.byType(TerminalPage))).pop();
      await tester.pumpAndSettle();
      await tester.tap(
        find.byWidgetPredicate(
          (widget) => widget is RoundButton && widget.icon == 'terminal',
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text(unavailable), findsOneWidget);
      expect(find.byType(TerminalPage), findsOneWidget);
      expect(find.byType(TerminalToolbar), findsNothing);
      expect(
        calls.where((kind) => kind == 'read_terminal_settings'),
        hasLength(2),
      );
      expect(calls, isNot(contains('create_terminal')));
      expect(calls, isNot(contains('open_terminal')));
      expect(connection.watched, isEmpty);
      expect(host.connected, isTrue);
      expect(tester.takeException(), isNull);
      await _release(tester, session);
    },
  );

  testWidgets(
    'closing a project terminal releases its observation and leaves the shell running',
    (tester) async {
      final connection = _TerminalConnection();
      final calls = <String>[];
      final host = _host(connection, (kind, data) async {
        calls.add(kind);
        if (kind == 'open_terminal') {
          expect(data?['terminal'], 'shell');
          return {'data': _terminal};
        }
        return {'data': {}};
      });
      final session = AppSession.test(hosts: [host]);
      final context = await _openProject(tester, session);
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
