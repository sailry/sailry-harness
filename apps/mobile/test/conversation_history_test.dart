import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/message_editor.dart';
import 'package:sailry_mobile/features/conversations/live/turn_actions.dart';
import 'package:sailry_mobile/features/conversations/user_message.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'live_conversations_test.dart' as fixture;

Map<String, dynamic> history({String status = 'completed', int revision = 12}) {
  final value = fixture.view();
  final page = value['snapshot']['page'] as Map;
  page['revision'] = revision;
  page['runs'] = [
    {'turn': 'first', 'status': status},
    {'turn': 'last', 'status': 'completed'},
  ];
  page['entries'] = [
    {
      'id': 'first-user',
      'turn': 'first',
      'author': 'user',
      'parts': [
        {'kind': 'text', 'data': 'Original input'},
        {
          'kind': 'attachment',
          'data': {
            'id': 'attachment',
            'spec': {'name': 'brief.txt', 'media_type': 'text/plain'},
          },
        },
        {
          'kind': 'reference',
          'data': {
            'target': {'kind': 'file', 'data': 'source.rs'},
            'label': 'Source',
          },
        },
      ],
    },
    {
      ...fixture.entry('answer', 'assistant', 'Original answer'),
      'turn': 'first',
    },
    {...fixture.entry('last-user', 'user', 'Last input'), 'turn': 'last'},
    {
      ...fixture.entry('last-answer', 'assistant', 'Last answer'),
      'turn': 'last',
    },
  ];
  return value;
}

Map<String, dynamic> replacement() => {
  'kind': 'turn_replaced',
  'data': {
    'turn': {'id': 'replacement'},
    'history': {'backup': fixture.session(id: 'backup')},
  },
};

class HistoryConnection extends fixture.ConnectionFixture {
  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
  }) async {
    if (session == 'session') return super.watchConversation(session: session);
    watched.add(session);
    final other = fixture.UpdatesFixture();
    other.emit(fixture.view());
    return other;
  }

  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    return jsonEncode({'Ok': replacement()});
  }
}

class Harness {
  Harness({CommandHandler? command}) {
    host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (kind, data) async {
        calls.add((kind, data!));
        return command == null ? replacement() : command(kind, data);
      },
    );
    app = AppSession.test(hosts: [host]);
  }
  final record = fixture.session();
  final connection = HistoryConnection();
  final calls = <(String, Map<String, dynamic>)>[];
  late final HostConnection host;
  late final AppSession app;
  Future<void> mount(WidgetTester tester, Map<String, dynamic> value) async {
    await fixture.mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    connection.updates.emit(value);
    await tester.pumpAndSettle();
  }

  void action(WidgetTester tester, String action, String turn) => tester
      .widget<LiveTimeline>(find.byType(LiveTimeline))
      .onTurnAction!(action, turn);
  Future<void> close(WidgetTester tester) async {
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  }
}

void main() {
  testWidgets(
    'edited replacement preserves input ownership and both revisions',
    (tester) async {
      final harness = Harness();
      await harness.mount(tester, history());
      harness.action(tester, 'edit', 'first');
      await tester.pumpAndSettle();
      expect(find.byType(MessageEditor), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'Changed input');
      await tester.tap(find.text(tr('messageRegenerate')));
      await tester.pumpAndSettle();
      expect(harness.calls.single.$1, 'replace_turn');
      expect(harness.calls.single.$2, {
        'session': 'session',
        'turn': 'first',
        'expected_head': 'last',
        'expected_history_revision': 12,
        'expected_revision': 7,
        'message': {
          'text': 'Changed input',
          'attachments': ['attachment'],
          'references': [
            {
              'target': {'kind': 'file', 'data': 'source.rs'},
              'label': 'Source',
            },
          ],
        },
      });
      expect(find.byType(MessageEditor), findsNothing);
      expect(find.text(tr('messageBackup')), findsOneWidget);
      await harness.close(tester);
    },
  );

  testWidgets('revision change retains the edit without sending', (
    tester,
  ) async {
    final harness = Harness();
    await harness.mount(tester, history());
    harness.action(tester, 'edit', 'first');
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Keep this edit');
    harness.connection.updates.emit(history(revision: 13));
    await tester.pump();
    await tester.tap(find.text(tr('messageRegenerate')));
    await tester.pumpAndSettle();
    expect(harness.calls, isEmpty);
    expect(find.text('Keep this edit'), findsOneWidget);
    await tester.tap(find.text(tr('messageEdit')));
    await tester.pumpAndSettle();
    harness.action(tester, 'edit', 'first');
    await tester.pumpAndSettle();
    expect(find.text('Keep this edit'), findsOneWidget);
    await tester.tap(find.text(tr('messageRegenerate')));
    await tester.pumpAndSettle();
    expect(harness.calls.single.$2['expected_history_revision'], 13);
    await harness.close(tester);
  });

  testWidgets(
    'unknown edit reconciles once without replacing the draft request',
    (tester) async {
      final gate = Completer<Map<String, dynamic>>();
      final harness = Harness(command: (_, _) => gate.future);
      await harness.mount(tester, history());
      harness.action(tester, 'edit', 'first');
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'Once only');
      await tester.tap(find.text(tr('messageRegenerate')));
      await tester.pump();
      expect(
        tester
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, tr('messageRegenerate')),
            )
            .onPressed,
        isNull,
      );
      expect(harness.calls, hasLength(1));
      gate.completeError(
        const CommandFailure(
          'outcome_unknown',
          request: 'original-edit-request',
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.widget<TextField>(find.byType(TextField)).enabled, isFalse);
      await tester.tap(find.text(tr('messageCheck')));
      await tester.pumpAndSettle();
      expect(harness.calls, hasLength(1));
      expect(harness.connection.reconciled, ['original-edit-request']);
      expect(find.byType(MessageEditor), findsNothing);
      await harness.close(tester);
    },
  );

  testWidgets('failed retry uses the original complete input', (tester) async {
    final harness = Harness(
      command: (_, _) async => {
        'kind': 'queued_turn',
        'data': {'id': 'retry'},
      },
    );
    await harness.mount(tester, history(status: 'failed'));
    final retry = find.descendant(
      of: find.byType(TurnActions).first,
      matching: find.byTooltip(tr('retry')),
    );
    await tester.ensureVisible(retry);
    await tester.tap(retry);
    await tester.pumpAndSettle();
    expect(harness.calls.single.$1, 'submit_turn');
    expect(harness.calls.single.$2['expected_revision'], 7);
    expect(harness.calls.single.$2['message']['text'], 'Original input');
    expect(harness.calls.single.$2['message']['attachments'], ['attachment']);
    expect(harness.calls.single.$2['message']['references'], hasLength(1));
    await harness.close(tester);
  });

  testWidgets('message menu forks the selected turn and opens the result', (
    tester,
  ) async {
    final harness = Harness(
      command: (_, _) async => {
        'kind': 'session',
        'data': fixture.session(id: 'branch'),
      },
    );
    await harness.mount(tester, history());
    final more = find.byTooltip(tr('messageActions')).first;
    await tester.ensureVisible(more);
    await tester.tap(more);
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('fork')));
    await tester.pumpAndSettle();
    expect(harness.calls.single.$1, 'fork_conversation');
    expect(harness.calls.single.$2, {
      'session': 'session',
      'through': 'first',
      'expected_revision': 7,
    });
    expect(harness.connection.watched, ['session', 'branch']);
    await harness.close(tester);
  });

  testWidgets('rewind is confirmed and preserves the history backup', (
    tester,
  ) async {
    final harness = Harness(
      command: (_, _) async => {
        'kind': 'rewound',
        'data': {'backup': fixture.session(id: 'backup')},
      },
    );
    await harness.mount(tester, history());
    harness.action(tester, 'rewind', 'first');
    await tester.pumpAndSettle();
    expect(harness.calls, isEmpty);
    await tester.tap(find.text(tr('confirm')));
    await tester.pumpAndSettle();
    expect(harness.calls.single.$1, 'rewind_conversation');
    expect(harness.calls.single.$2, {
      'session': 'session',
      'through': 'first',
      'expected_head': 'last',
      'expected_revision': 12,
    });
    await tester.tap(find.text(tr('messageBackup')));
    await tester.pumpAndSettle();
    expect(harness.connection.watched, ['session', 'backup']);
    await harness.close(tester);
  });

  testWidgets('system turns cannot submit an empty regenerated message', (
    tester,
  ) async {
    final harness = Harness();
    final value = history(status: 'failed');
    (value['snapshot']['page']['entries'] as List).removeWhere(
      (entry) => entry['turn'] == 'first' && entry['author'] == 'user',
    );
    await harness.mount(tester, value);
    harness.action(tester, 'edit', 'first');
    harness.action(tester, 'retry', 'first');
    await tester.pumpAndSettle();
    expect(harness.calls, isEmpty);
    expect(find.byType(MessageEditor), findsNothing);
    final actions = tester.widget<TurnActions>(find.byType(TurnActions).first);
    expect(actions.canEdit, isFalse);
    expect(actions.canRewind, isTrue);
    await harness.close(tester);
  });

  for (final blocked in [
    'running',
    'queued',
    'delegated',
    'archived',
    'disconnected',
  ]) {
    testWidgets('$blocked sessions cannot change history', (tester) async {
      final harness = Harness();
      final value = history();
      if (blocked == 'running') {
        value['snapshot']['page']['runs'][1]['status'] = 'running';
      }
      if (blocked == 'queued') {
        value['snapshot']['page']['queue']['items'] = [
          {'turn': 'queued', 'preview': 'Queued'},
        ];
      }
      if (blocked == 'delegated') {
        harness.record['delegation'] = {'parent': 'parent'};
      }
      if (blocked == 'archived') harness.record['archived'] = true;
      if (blocked == 'disconnected') value['connected'] = false;
      await harness.mount(tester, value);
      expect(
        tester.widget<LiveTimeline>(find.byType(LiveTimeline)).canChangeHistory,
        isFalse,
      );
      expect(
        tester
            .widgetList<UserBubble>(find.byType(UserBubble))
            .every((bubble) => bubble.onEdit == null),
        isTrue,
      );
      expect(harness.calls, isEmpty);
      if (blocked != 'disconnected') {
        harness.action(tester, 'edit', 'first');
        await tester.pumpAndSettle();
        expect(find.byType(MessageEditor), findsNothing);
        harness.action(tester, 'fork', 'first');
        await tester.pumpAndSettle();
        expect(harness.calls.single.$1, 'fork_conversation');
      }
      await harness.close(tester);
    });
  }
}
