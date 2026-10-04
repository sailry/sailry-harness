import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'live_conversations_test.dart' as fixture;

void main() {
  testWidgets('opening acknowledges the displayed attention revision', (
    tester,
  ) async {
    final connection = fixture.ConnectionFixture();
    final record = fixture.session();
    record['activity'] = {
      'title': 'Unread result',
      'attention': {'revision': 5, 'unread': true},
      'run': {'status': 'completed'},
    };
    final acknowledgements = <Map<String, dynamic>>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (kind, data) async {
        if (kind == 'set_session_read') acknowledgements.add(data!);
        return {};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      Scaffold(
        body: ConversationTask(host: host, session: record, project: const {}),
      ),
      app,
    );
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('task-unread-dot')), findsOneWidget);
    expect(find.byKey(const ValueKey('task-status-dot')), findsNothing);
    await tester.tap(find.text('Unread result'));
    await tester.pump();
    expect(acknowledgements, [
      {'session': 'session', 'expected_revision': 5, 'read': true},
    ]);
    connection.updates.emit(fixture.view());
    await tester.pumpAndSettle();
    // A later completion arriving while the conversation is open stays unread.
    record['activity']['attention'] = {'revision': 6, 'unread': true};
    connection.updates.emit(fixture.view());
    await tester.pumpAndSettle();
    expect(acknowledgements, hasLength(1));
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('subscription updates replace the trailing unread dot', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    Future<void> render(bool unread, String status) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ConversationTask(
              host: host,
              project: const {},
              session: {
                ...fixture.session(),
                'activity': {
                  'title': 'Task',
                  'attention': {'revision': 2, 'unread': unread},
                  'run': {'status': status},
                },
              },
            ),
          ),
        ),
      );
      await tester.pump();
    }

    await render(true, 'completed');
    expect(find.byKey(const ValueKey('task-unread-dot')), findsOneWidget);
    await render(false, 'completed');
    expect(find.byKey(const ValueKey('task-unread-dot')), findsNothing);
    await render(true, 'running');
    expect(find.byKey(const ValueKey('task-unread-dot')), findsNothing);
    final spinner = find.byKey(const ValueKey('task-loading'));
    expect(spinner, findsOneWidget);
    expect(
      tester.getCenter(spinner).dx,
      greaterThan(tester.getCenter(find.text('Task')).dx),
    );
    await tester.pumpWidget(const SizedBox());
    await host.close();
  });
}
