import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/conversations/live/terminal_tasks.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/projects_page.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'live_terminal_test.dart' as terminal;

HostConnection host(
  terminal.ConnectionFixture connection,
  List<String> commands,
) => HostConnection.test(
  id: 'host',
  label: 'A very long desktop device name that must fit on one line',
  connection: connection,
  snapshot: {
    'projects': [
      {'id': 'project', 'name': 'Project', 'path': '/project'},
    ],
    'worktrees': [
      {'id': 'tree', 'project': 'project', 'path': '/project'},
    ],
    'sessions': [
      {
        'id': 'session',
        'project': 'project',
        'worktree': 'tree',
        'revision': 1,
        'activity': {
          'title': 'Chat task',
          'run': {'status': 'completed'},
        },
      },
    ],
    'terminals': [
      {
        'id': 'existing',
        'title': 'Shell task',
        'worktree': 'tree',
        'status': {'kind': 'running'},
        'revision': 1,
      },
    ],
  },
  command: (kind, data) async {
    if (kind == 'read_conversation') {
      return {
        'data': {
          'page': {'entries': []},
          'missing': [],
        },
      };
    }
    commands.add(kind);
    if (kind == 'open_terminal') {
      expect(data!['terminal'], 'existing');
      expect(data['viewport'], isNotNull);
      expect(data['appearance'], isNotNull);
      return {
        'kind': 'terminal',
        'data': {'id': 'existing', 'revision': 1},
      };
    }
    if (kind == 'create_terminal') {
      expect(data!['worktree'], 'tree');
      return {
        'data': {'id': 'new-terminal', 'revision': 1},
      };
    }
    expect(kind, 'read_host_metrics');
    return {
      'data': {
        'cpu_basis_points': 2000,
        'processes': [
          {'name': 'worker', 'cpu_basis_points': 1000, 'memory_bytes': 1024},
        ],
      },
    };
  },
)..info = {'os': 'Darwin', 'architecture': 'arm64'};

void main() {
  testWidgets('compact identity and statistics destinations', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    final connection = terminal.ConnectionFixture();
    final commands = <String>[];
    final node = host(connection, commands);
    final app = AppSession.test(hosts: [node]);
    await tester.pumpWidget(SailryApp(session: app));
    await tester.pumpAndSettle();
    expect(find.text('Chat task'), findsOneWidget);
    expect(find.text('Shell task'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    final label = tester.widget<Text>(find.text(node.label));
    expect(label.maxLines, 1);
    expect(label.overflow, TextOverflow.ellipsis);
    expect(label.style!.fontSize, 16);
    expect(
      tester.getCenter(find.text('Darwin · arm64')).dy,
      closeTo(tester.getCenter(find.text(tr('online'))).dy, 1),
    );
    expect(find.text(tr('hostRefresh')), findsNothing);
    expect(find.text(tr('hostRegisterProject')), findsNothing);
    expect(find.text(tr('process')), findsNothing);
    await tester.tap(find.byKey(const ValueKey('host-projects')));
    await tester.pumpAndSettle();
    expect(find.byType(ProjectsPage), findsOneWidget);
    expect(find.widgetWithText(ListTile, 'Project'), findsOneWidget);
    Navigator.pop(tester.element(find.byType(ProjectsPage)));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('host-sessions')));
    await tester.pumpAndSettle();
    expect(find.text('Chat task'), findsOneWidget);
    expect(find.byType(TerminalTask), findsNothing);
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('host-terminals')));
    await tester.pumpAndSettle();
    expect(find.text('Chat task'), findsNothing);
    expect(find.text('Shell task'), findsOneWidget);
    await tester.tap(find.byType(TerminalTask));
    await tester.pumpAndSettle();
    expect(connection.watched, ['existing']);
    expect(commands.where((kind) => kind == 'open_terminal'), hasLength(1));
    expect(commands.where((kind) => kind == 'create_terminal'), isEmpty);
    expect(
      tester.widget<TerminalPage>(find.byType(TerminalPage)).terminalId,
      'existing',
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('creates a terminal beside an existing one', (tester) async {
    final connection = terminal.ConnectionFixture();
    final commands = <String>[];
    final node = host(connection, commands);
    final app = AppSession.test(hosts: [node]);
    await tester.pumpWidget(
      SessionScope(
        session: app,
        child: const MaterialApp(home: TasksPage()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('newTerminal')));
    await tester.pumpAndSettle();
    expect(commands, isEmpty);
    await tester.tap(find.widgetWithText(ListTile, 'Project'));
    await tester.pumpAndSettle();
    expect(commands, ['create_terminal']);
    expect(connection.watched, ['new-terminal']);
    expect(
      tester.widget<TerminalPage>(find.byType(TerminalPage)).createNew,
      isTrue,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });
}
