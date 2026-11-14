import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/question.dart';
import 'package:sailry_mobile/features/conversations/live/tool_record.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

void main() {
  Future<void> openReview(
    WidgetTester tester, {
    required bool completed,
    required Future<Map<String, dynamic>> Function(String, Map<String, dynamic>)
    command,
    required Future<void> Function(String, String) feedback,
    String? Function(String)? pending,
  }) async {
    final question = {
      'id': 'question',
      'turn': 'turn',
      'state': {'kind': 'pending'},
    };
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ToolRecord(
            enabled: true,
            call: {
              'name': completed ? 'plan_review' : 'ask_user',
              'turn': 'turn',
              'state': 'waiting',
              'source': {'entry': 'review', 'index': 0},
              'question': question,
            },
            page: {
              'runs': [
                {
                  'turn': 'turn',
                  'kind': 'task',
                  'status': completed ? 'completed' : 'running',
                },
              ],
              'questions': [question],
              'entries': [
                {
                  'id': 'review',
                  'author': completed ? 'plan_review' : 'assistant',
                  'parts': [
                    {
                      'kind': 'tool_call',
                      'data': {
                        'summary': {
                          'question': {
                            'prompt': completed ? '' : 'Review this plan',
                            'input': {'kind': 'plan'},
                          },
                        },
                      },
                    },
                  ],
                },
              ],
            },
            session: const {
              'id': 'session',
              'revision': 7,
              'config': {'mode': 'plan'},
            },
            command: command,
            onPlanFeedback: feedback,
            pendingPlanFeedback: pending,
          ),
        ),
      ),
    );
    await tester.tap(find.text(tr('reply')));
    await tester.pumpAndSettle();
  }

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

  testWidgets('empty completed-plan feedback only closes the review', (
    tester,
  ) async {
    final sent = <(String, Map<String, dynamic>)>[];
    await openReview(
      tester,
      completed: true,
      command: (kind, data) async {
        sent.add((kind, data));
        return {};
      },
      feedback: (_, _) async => fail('Empty feedback queued another turn'),
    );
    await tester.tap(find.text(tr('conversationSubmit')));
    await tester.pumpAndSettle();
    expect(sent.map((entry) => [entry.$1, entry.$2]), [
      [
        'resolve_question',
        {
          'session': 'session',
          'question': 'question',
          'response': {
            'kind': 'answer',
            'data': {'kind': 'text', 'data': ''},
          },
        },
      ],
    ]);
  });

  testWidgets('running plan feedback answers the waiting model question', (
    tester,
  ) async {
    final sent = <(String, Map<String, dynamic>)>[];
    await openReview(
      tester,
      completed: false,
      command: (kind, data) async {
        sent.add((kind, data));
        return {};
      },
      feedback: (_, _) async => fail('Active question queued another turn'),
    );
    await tester.enterText(find.byType(TextField), 'Revise step two');
    await tester.tap(find.text(tr('conversationSubmit')));
    await tester.pumpAndSettle();
    expect(sent.map((entry) => [entry.$1, entry.$2]), [
      [
        'resolve_question',
        {
          'session': 'session',
          'question': 'question',
          'response': {
            'kind': 'answer',
            'data': {'kind': 'text', 'data': 'Revise step two'},
          },
        },
      ],
    ]);
    expect(find.byType(ConversationQuestion), findsNothing);
  });

  testWidgets('completed plan feedback retains and checks the original send', (
    tester,
  ) async {
    final connection = RecoveryConnection()
      ..result = {
        'data': {'id': 'planning-turn'},
      };
    final sent = <(String, Map<String, dynamic>)>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      connection: connection,
      command: (kind, data) async {
        sent.add((kind, data!));
        throw const CommandFailure('outcome_unknown', request: 'original');
      },
    );
    final draft = TurnDraft(
      host: host,
      session: 'session',
      text: 'Unsent input',
    );
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    await openReview(
      tester,
      completed: true,
      command: (_, _) async => fail('Completed feedback only closed review'),
      feedback: (question, message) async {
        if (draft.pending != null) {
          expect(draft.pending!.question, question);
          await draft.check();
        } else {
          await draft.send(7, message: message, question: question);
        }
      },
      pending: (question) =>
          draft.pending?.question == question ? draft.pending?.text : null,
    );
    await tester.enterText(find.byType(TextField), 'Revise step two');
    await tester.tap(find.text(tr('conversationSubmit')));
    await tester.pumpAndSettle();
    expect(sent.map((entry) => [entry.$1, entry.$2]), [
      [
        'submit_turn',
        {
          'session': 'session',
          'expected_revision': 7,
          'message': {'text': 'Revise step two', 'attachments': []},
        },
      ],
    ]);
    expect(draft.pending?.question, 'question');
    expect(tester.widget<TextField>(find.byType(TextField)).enabled, isFalse);
    expect(find.text(tr('conversationCheckResult')), findsOneWidget);
    expect(
      tester
          .widget<FilledButton>(
            find.widgetWithText(FilledButton, tr('conversationStartCoding')),
          )
          .onPressed,
      isNull,
    );

    // Closing the sheet does not discard the page-owned retained request.
    Navigator.of(tester.element(find.byType(ConversationQuestion))).pop();
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('reply')));
    await tester.pumpAndSettle();
    final input = tester.widget<TextField>(find.byType(TextField));
    expect(input.controller!.text, 'Revise step two');
    expect(input.enabled, isFalse);
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pumpAndSettle();
    expect(connection.requests, ['original']);
    expect(sent.length, 1);
    expect(draft.pending, isNull);
    expect(draft.input.text, 'Unsent input');
    expect(find.byType(ConversationQuestion), findsNothing);
  });

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
