import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/attachment.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/turn_frame.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

void main() {
  setUp(
    () =>
        TestWidgetsFlutterBinding.ensureInitialized()
            .platformDispatcher
            .accessibilityFeaturesTestValue = const FakeAccessibilityFeatures(
          disableAnimations: true,
        ),
  );
  tearDown(
    () => TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher
        .clearAccessibilityFeaturesTestValue(),
  );
  testWidgets('retry counters and terminal reasons retain the answer', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    Map<String, dynamic> retry(String id, int attempt) => {
      'id': id,
      'author': 'system',
      'turn': 'turn',
      'parts': [
        {
          'kind': 'resource',
          'data': {'type': 'model_retry', 'attempt': attempt, 'limit': 5},
        },
      ],
    };
    String counter(String key, int attempt) =>
        tr(key).replaceAll('{attempt}', '$attempt').replaceAll('{limit}', '5');
    Future<void> render({bool failed = false, bool connected = true}) async {
      final view = fixture.view(
        status: failed ? 'failed' : 'running',
        connected: connected,
        entries: [
          retry('retry-a', 1),
          retry('retry-b', 2),
          if (failed)
            fixture.entry('answer', 'assistant', 'Partial answer remains'),
        ],
      );
      if (failed) {
        view['snapshot']['page']['runs'] = [
          {
            ...(view['snapshot']['page']['runs'] as List).single,
            'error': {
              'code': 'unavailable',
              'message': 'Provider request timed out',
            },
          },
        ];
      }
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: SingleChildScrollView(
              child: LiveTimeline(
                view: view,
                session: fixture.session(),
                host: host,
                command: (_, _) async => {},
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
    }

    await render();
    expect(find.text(counter('conversationModelRetrying', 2)), findsOneWidget);
    expect(find.text(counter('conversationModelRetrying', 1)), findsNothing);
    expect(find.text(tr('thinkingNow')), findsNothing);
    await render(connected: false);
    expect(find.text(tr('conversationUnsynced')), findsOneWidget);
    expect(find.text(counter('conversationModelRetrying', 2)), findsNothing);
    expect(find.text(counter('conversationModelRetried', 2)), findsOneWidget);
    await render(failed: true);
    expect(find.text('Partial answer remains'), findsOneWidget);
    expect(find.text('Provider request timed out'), findsNothing);
    await tester.tap(find.text(tr('conversationErrorDetails')));
    await tester.pumpAndSettle();
    expect(find.text('Provider request timed out'), findsOneWidget);
    await tester.tap(find.text(tr('conversationFailedStatus')));
    await tester.pumpAndSettle();
    expect(find.text(counter('conversationModelRetried', 2)), findsOneWidget);
    expect(find.text('Partial answer remains'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    host.dispose();
  });

  test('Node duration freezes on completion', () {
    final run = {'status': 'running', 'started_ms': 1000};
    expect(elapsed(run, 13000), '12s');
    expect(elapsed(run, 72000), '1m11s');
    expect(elapsed(run, 3603000), '1h00m02s');
    expect(elapsed(run, 0), '0s');
    expect(
      elapsed({...run, 'status': 'completed', 'finished_ms': 18000}, 999999),
      '17s',
    );
    expect(elapsed({'status': 'queued'}, 99999), isNull);
  });

  testWidgets('tool lifecycle and process-only folding', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final thought = {
      'id': 'thought',
      'author': 'assistant',
      'turn': 'turn',
      'parts': [
        {'kind': 'thinking', 'data': 'Thinking detail'},
      ],
    };
    final tool = {
      'id': 'tool',
      'author': 'assistant',
      'turn': 'turn',
      'parts': [
        {
          'kind': 'tool_call',
          'data': {
            'arguments': {'path': 'source.txt'},
          },
        },
        {
          'kind': 'tool_result',
          'data': {'result': 'Tool output'},
        },
      ],
    };
    Future<void> render(
      String status, {
      String? toolState,
      bool answer = false,
      bool connected = true,
      bool settle = true,
    }) async {
      final projection = fixture.view(
        status: status,
        connected: connected,
        entries: [
          thought,
          if (toolState != null) tool,
          if (answer)
            fixture.entry('answer-a', 'assistant', 'First final paragraph'),
          if (answer)
            fixture.entry('answer-b', 'assistant', 'Second final paragraph'),
        ],
        calls: [
          if (toolState != null)
            {
              'turn': 'turn',
              'name': 'read_file',
              'state': toolState,
              'source': {'entry': 'tool', 'index': 0},
              if (toolState == 'returned')
                'response': {'entry': 'tool', 'index': 1},
            },
        ],
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: SingleChildScrollView(
              child: LiveTimeline(
                view: projection,
                session: fixture.session(),
                host: host,
                command: (_, _) async => {},
              ),
            ),
          ),
        ),
      );
      if (settle) await tester.pumpAndSettle();
    }

    await render('running');
    expect(find.text(tr('thinkingNow')), findsOneWidget);
    expect(find.text('Thinking detail'), findsNothing);
    expect(find.text(tr('workProcess')), findsNothing);
    await render('running', toolState: 'running');
    expect(find.text('Thinking detail'), findsNothing);
    expect(find.textContaining('source.txt'), findsOneWidget);
    await render('running', toolState: 'returned');
    expect(find.textContaining('source.txt'), findsOneWidget);
    expect(find.text('Tool output'), findsNothing);
    tester.binding.platformDispatcher.clearAccessibilityFeaturesTestValue();
    await render(
      'completed',
      toolState: 'returned',
      answer: true,
      settle: false,
    );
    expect(find.textContaining(tr('tool_read_file')), findsOneWidget);
    await tester.pumpAndSettle();
    expect(find.textContaining(tr('tool_read_file')), findsNothing);
    expect(find.text('First final paragraph'), findsOneWidget);
    expect(find.text('Second final paragraph'), findsOneWidget);
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    expect(find.textContaining(tr('tool_read_file')), findsOneWidget);
    await tester.tap(find.textContaining(tr('tool_read_file')));
    await tester.pumpAndSettle();
    expect(find.text('Tool output'), findsOneWidget);
    await render('running', connected: false);
    expect(find.text(tr('conversationUnsynced')), findsOneWidget);
    expect(find.text(tr('thinkingNow')), findsNothing);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });

  testWidgets('mixed content', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final content = {
      'version': 1,
      'blocks': [
        {
          'kind': 'table',
          'columns': ['File', 'State'],
          'rows': [
            ['hello.txt', 'Updated'],
          ],
        },
        {
          'kind': 'diff',
          'path': 'hello.txt',
          'text': '--- a/hello.txt\n+++ b/hello.txt\n-Hello\n+Updated\n',
        },
        {'kind': 'image', 'index': 0},
      ],
    };
    final payloads = [
      {'sailry_content': content},
      {
        'sailry_content': {'version': 1, 'blocks': 'invalid'},
        'text': 'Unformatted output',
      },
      {'error': 'Tool failed', 'text': 'Partial output'},
    ];
    for (final payload in payloads) {
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: SingleChildScrollView(
              child: LiveTimeline(
                view: fixture.view(
                  entries: [
                    {
                      'id': 'tool',
                      'author': 'assistant',
                      'turn': 'turn',
                      'parts': [
                        {
                          'kind': 'tool_call',
                          'data': {'arguments': <String, dynamic>{}},
                        },
                        {
                          'kind': 'tool_result',
                          'data': {
                            'result': payload,
                            'images': [
                              {
                                'index': 0,
                                'attachment': {
                                  'id': 'image',
                                  'spec': {
                                    'name': 'preview.png',
                                    'media_type': 'image/png',
                                    'size': 4,
                                  },
                                },
                              },
                            ],
                          },
                        },
                      ],
                    },
                  ],
                  calls: [
                    {
                      'turn': 'turn',
                      'name': 'inspect_files',
                      'state': 'returned',
                      'presentation': 'content',
                      if (payload == payloads.first) 'content': content,
                      'source': {'entry': 'tool', 'index': 0},
                      'response': {'entry': 'tool', 'index': 1},
                    },
                  ],
                ),
                session: fixture.session(),
                host: host,
                command: (_, _) async => {},
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('completed')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('inspect_files'));
      await tester.pumpAndSettle();
      expect(find.byType(AttachmentView), findsOneWidget);
      expect(find.text('preview.png'), findsOneWidget);
      expect(find.text(tr('toolRaw')), findsNothing);
      if (payload == payloads.first) {
        expect(find.text('hello.txt'), findsWidgets);
        expect(find.text('Updated'), findsOneWidget);
      } else if (!payload.containsKey('error')) {
        expect(
          find.text(const JsonEncoder.withIndent('  ').convert(payload)),
          findsOneWidget,
        );
      }
      expect(
        find.text('Tool failed'),
        payload.containsKey('error') ? findsOneWidget : findsNothing,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    }
    await host.close();
    host.dispose();
  });

  testWidgets('composer grows above fixed actions', (tester) async {
    final controller = TextEditingController();
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: Align(
            alignment: Alignment.bottomCenter,
            child: MessageComposer(
              controller: controller,
              attached: false,
              busy: false,
              onAttachment: (_) {},
              onVoice: () {},
              onStop: () {},
              onSend: () {},
            ),
          ),
        ),
      ),
    );
    final input = find.byKey(const ValueKey('composer-input'));
    final initial = tester.getRect(input);
    final send = tester.getRect(find.byTooltip(tr('send')));
    expect(tester.getCenter(find.byTooltip(tr('attach'))).dy, send.center.dy);
    expect(tester.getCenter(find.byTooltip(tr('voice'))).dy, send.center.dy);
    await tester.enterText(find.byType(TextField), 'One\nTwo\nThree\nFour');
    await tester.pumpAndSettle();
    final expanded = tester.getRect(input);
    expect(expanded.height, greaterThan(initial.height));
    expect(expanded.bottom, initial.bottom);
    expect(tester.getRect(find.byTooltip(tr('send'))).bottom, send.bottom);
    expect(tester.getCenter(find.byTooltip(tr('attach'))).dy, send.center.dy);
    expect(find.byTooltip(tr('modelPicker')), findsNothing);
    await tester.enterText(find.byType(TextField), '');
    await tester.pumpAndSettle();
    expect(tester.getRect(input).height, initial.height);
    await tester.pumpWidget(const SizedBox());
    controller.dispose();
  });

  testWidgets(
    'message menu dismissal does not fork and failure remains visible',
    (tester) async {
      final connection = fixture.ConnectionFixture();
      final calls = <Map<String, dynamic>?>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        command: (kind, data) async {
          expect(kind, 'fork_conversation');
          calls.add(data);
          throw const CommandFailure('revision_conflict');
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: fixture.session(),
        ),
        app,
      );
      connection.updates.emit(fixture.view());
      await tester.pumpAndSettle();
      expect(find.byTooltip(tr('more')), findsNothing);
      expect(find.text('Actual task'), findsNothing);
      expect(find.byTooltip(tr('modelPicker')), findsOneWidget);
      await tester.tap(find.byTooltip(tr('messageActions')));
      await tester.pumpAndSettle();
      expect(calls, isEmpty);
      await tester.tapAt(const Offset(5, 5));
      await tester.pumpAndSettle();
      expect(calls, isEmpty);
      await tester.tap(find.byTooltip(tr('messageActions')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('fork')));
      await tester.pumpAndSettle();
      expect(calls.single, {
        'session': 'session',
        'through': 'turn',
        'expected_revision': 7,
      });
      expect(find.text(tr('conversationConflict')), findsOneWidget);
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
    },
  );
}
