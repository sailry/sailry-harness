import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/demo/connection.dart';
import 'package:sailry_mobile/features/conversations/live/conversation_terminals.dart';
import 'package:sailry_mobile/features/conversations/live/terminal_tasks.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/swipe_action.dart';

Future<Map<String, dynamic>> command(
  DemoConnection connection,
  String kind,
  Map<String, dynamic> data,
) async {
  final request = await connection.prepare(
    command: jsonEncode({'kind': kind, 'data': data}),
  );
  return object(jsonDecode(await connection.execute(request: request)));
}

Widget screen(AppSession session, Widget home) => SessionScope(
  session: session,
  child: MaterialApp(
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: home,
  ),
);

Map<String, dynamic> terminal(
  String id, {
  String worktree = 'scratch',
  String status = 'running',
  Object? tool,
}) => {
  'id': id,
  'title': '$id-title',
  'worktree': worktree,
  'access': 'unrestricted',
  'tool': tool,
  'status': {'kind': status},
  'revision': 1,
};

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('demo terminal closes and a new terminal starts it again', () async {
    final connection = DemoConnection(translate: tr);
    expect(objects(connection.snapshot['terminals']), hasLength(1));
    final closed = await command(connection, 'close_terminal', {
      'worktree': 'demo-lumen-main',
      'terminal': 'demo-terminal',
    });
    expect(closed, contains('Ok'));
    expect(objects(connection.snapshot['terminals']), isEmpty);
    final reopened = await command(connection, 'open_terminal', {
      'terminal': 'demo-terminal',
    });
    expect(reopened, isNot(contains('Ok')));
    final created = await command(connection, 'create_terminal', {
      'worktree': 'demo-lumen-main',
      'access': 'unrestricted',
    });
    expect(created, contains('Ok'));
    expect(
      objects(connection.snapshot['terminals']).map((value) => value['id']),
      ['demo-terminal'],
    );
    await connection.close();
  });

  testWidgets('terminal page ends its shell after confirmation', (
    tester,
  ) async {
    final session = AppSession.test(
      initialize: () async => fail('Terminal opened native transport'),
    );
    final host = await session.pairPin('888888');
    await tester.pumpWidget(
      screen(
        session,
        Builder(
          builder: (context) => TextButton(
            onPressed: () => pushPage(
              context,
              TerminalPage(
                hostId: host.id,
                worktreeId: 'demo-lumen-main',
                title: 'zsh',
              ),
            ),
            child: const Text('open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(TerminalPage), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('terminal-end')));
    await tester.pumpAndSettle();
    expect(find.text('Its shell and running commands stop'), findsOneWidget);
    expect(objects(host.snapshot['terminals']), hasLength(1));

    await tester.tap(find.byKey(const ValueKey('terminal-end-confirm')));
    await tester.pumpAndSettle();
    expect(find.byType(TerminalPage), findsNothing);
    expect(objects(host.snapshot['terminals']), isEmpty);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });

  testWidgets('conversation terminals list, end and start shells in its '
      'worktree', (tester) async {
    final calls = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'worktrees': [
          {'id': 'scratch', 'project': null, 'path': '/scratch'},
        ],
        'terminals': [
          terminal('mine'),
          terminal('elsewhere', worktree: 'other'),
          terminal('ended', status: 'exited'),
          terminal('tool', tool: 'codex'),
        ],
      },
      command: (kind, data) async {
        calls.add((kind, data));
        return {'data': <String, dynamic>{}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(
      screen(
        session,
        Scaffold(
          body: ConversationTerminals(host: host, worktree: 'scratch'),
        ),
      ),
    );
    final rows = tester
        .widgetList<TerminalTask>(find.byType(TerminalTask))
        .map((row) => row.terminal['id'])
        .toList();
    expect(rows, ['mine']);

    final row = tester.widget<Slidable>(
      find.byWidgetPredicate(
        (widget) =>
            widget is Slidable &&
            (widget.key as ValueKey?)?.value ==
                ('conversation-terminal', 'mine'),
      ),
    );
    final end = row.endActionPane!.children.single as SwipeAction;
    end.onPressed!(tester.element(find.byType(ConversationTerminals)));
    await tester.pump();
    expect(calls.single.$1, 'close_terminal');
    expect(calls.single.$2, {'worktree': 'scratch', 'terminal': 'mine'});

    host.snapshot['terminals'] = <dynamic>[];
    host.notifyListeners();
    await tester.pump();
    expect(find.byType(TerminalTask), findsNothing);
    expect(find.text('No running terminals'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('conversation-terminal-new')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 500));
    final page = tester.widget<TerminalPage>(find.byType(TerminalPage));
    expect(page.hostId, 'host');
    expect(page.worktreeId, 'scratch');
    expect(page.createNew, isTrue);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });
}
