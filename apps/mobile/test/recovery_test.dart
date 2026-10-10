import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'create_task_test.dart' as creation;
import 'live_conversations_test.dart' as live;

class RejectedRecovery extends live.ConnectionFixture {
  RejectedRecovery(this.code);
  final String code;
  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    return jsonEncode({
      'Err': {'code': code, 'message': 'Fixture conflict'},
    });
  }
}

Future<void> dismissFeedback(WidgetTester tester) async {
  for (var i = 0; i < 3; i++) {
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
  }
  await tester.pumpWidget(const SizedBox());
}

void main() {
  for (final (code, rejected) in const [
    ('revision_conflict', true),
    ('invalid_request', true),
    ('conflict', true),
    ('not_found', true),
    ('not_configured', true),
    ('outcome_unknown', false),
    ('unavailable', false),
    ('permission_denied', false),
    ('busy', false),
  ]) {
    testWidgets('creation recovery retains draft after $code', (tester) async {
      final connection = RejectedRecovery(code);
      var submissions = 0;
      final host = HostConnection.test(
        id: 'host-a',
        label: 'Host',
        snapshot: creation.snapshot('a'),
        connection: connection,
        command: (_, _) async {
          submissions++;
          throw const CommandFailure(
            'outcome_unknown',
            request: 'same-request',
          );
        },
      );
      final app = AppSession.test(hosts: [host]);
      addTearDown(() async {
        await app.close();
        app.dispose();
      });
      await creation.open(tester, app);
      await creation.submit(tester);
      expect(
        tester.widget<MessageComposer>(find.byType(MessageComposer)).enabled,
        isFalse,
      );
      await tester.tap(find.text(tr('conversationCheckResult')));
      await tester.pumpAndSettle();
      expect(connection.reconciled, ['same-request']);
      expect(find.text('First real task'), findsOneWidget);
      expect(
        tester.widget<MessageComposer>(find.byType(MessageComposer)).enabled,
        rejected,
      );
      if (!rejected) {
        await tester.tap(find.text(tr('conversationCheckResult')));
        await tester.pumpAndSettle();
        expect(connection.reconciled, ['same-request', 'same-request']);
      } else {
        expect(find.text(tr('conversationCheckResult')), findsNothing);
        await creation.submit(tester);
        expect(submissions, 2);
      }
      await dismissFeedback(tester);
    });
    testWidgets('send recovery permits correction after $code', (tester) async {
      final connection = RejectedRecovery(code);
      var submissions = 0;
      final record = live.session();
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        snapshot: {
          'sessions': [record],
        },
        command: (_, _) async {
          submissions++;
          throw const CommandFailure(
            'outcome_unknown',
            request: 'same-request',
          );
        },
      );
      final app = AppSession.test(hosts: [host]);
      addTearDown(() async {
        await app.close();
        app.dispose();
      });
      await live.mount(
        tester,
        LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: record,
        ),
        app,
      );
      connection.updates.emit(live.view());
      await tester.pump();
      await tester.pump();
      await tester.enterText(find.byType(TextField), 'Do this once');
      await tester.tap(find.byTooltip(tr('send')));
      await tester.pump();
      await tester.tap(find.text(tr('conversationCheckResult')));
      await tester.pumpAndSettle();
      expect(connection.reconciled, ['same-request']);
      expect(find.text('Do this once'), findsOneWidget);
      expect(
        find.text(tr('conversationCheckResult')),
        !rejected ? findsOneWidget : findsNothing,
      );
      if (!rejected) {
        await tester.tap(find.text(tr('conversationCheckResult')));
      } else {
        await tester.tap(find.byTooltip(tr('send')));
      }
      await tester.pumpAndSettle();
      if (!rejected) {
        expect(connection.reconciled, ['same-request', 'same-request']);
        expect(submissions, 1);
      } else {
        expect(submissions, 2);
      }
      await dismissFeedback(tester);
    });
  }
}
