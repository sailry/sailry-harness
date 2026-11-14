import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/question.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  Future<void> open(
    WidgetTester tester,
    String kind,
    Future<Map<String, dynamic>> Function(String, Map<String, dynamic>) command,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => AlertDialog(
                  content: ConversationQuestion(
                    spec: {
                      'prompt': 'Question',
                      'input': {
                        'kind': kind,
                        'options': <String>[],
                        'fields': <Map<String, dynamic>>[],
                        'url': 'https://example.com',
                      },
                    },
                    question: const {'id': 'question'},
                    session: const {'id': 'session', 'revision': 7},
                    command: command,
                  ),
                ),
              ),
              child: const Text('Open'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
  }

  for (final feedback in ['', 'Keep the existing API']) {
    testWidgets(
      'plan starts with valid input: ${feedback.isEmpty ? 'no feedback' : 'feedback'}',
      (tester) async {
        Map<String, dynamic>? requested;
        await open(tester, 'plan', (kind, data) async {
          expect(kind, 'resolve_question');
          requested = data;
          return {};
        });
        if (feedback.isNotEmpty) {
          await tester.enterText(find.byType(TextField), feedback);
        }
        await tester.tap(find.text(tr('conversationStartCoding')));
        await tester.pumpAndSettle();
        expect(requested, {
          'session': 'session',
          'question': 'question',
          'response': {
            'kind': 'start_coding',
            'data': {
              'expected_revision': 7,
              'message': {
                'text': feedback.isEmpty
                    ? tr('conversationStartCoding')
                    : feedback,
                'attachments': [],
              },
            },
          },
        });
      },
    );
  }

  testWidgets('decline is available only for MCP form and URL questions', (
    tester,
  ) async {
    for (final kind in ['text', 'choice', 'plan', 'form', 'url']) {
      await open(tester, kind, (_, _) async => {});
      expect(
        find.text(tr('deny')),
        ['form', 'url'].contains(kind) ? findsOneWidget : findsNothing,
      );
      await tester.tap(find.text(tr('cancel')));
      await tester.pumpAndSettle();
    }
  });
}
