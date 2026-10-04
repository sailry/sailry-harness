import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/content/diff.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';

import 'live_conversations_test.dart' as fixture;

void main() {
  WidgetController.hitTestWarningShouldBeFatal = true;
  final host = HostConnection.test(
    id: 'node',
    label: 'Node',
    command: (_, _) async => {},
  );
  final contracts = objects(
    jsonDecode(
      File('../../tests/fixtures/tool-display.json').readAsStringSync(),
    ),
  );

  Future<void> record(
    WidgetTester tester,
    Map<String, dynamic> contract, {
    Locale locale = const Locale('en'),
    bool pending = false,
  }) async {
    final name = text(contract['name']);
    final resolved = object(contract['resolved']);
    await tester.pumpWidget(
      MaterialApp(
        locale: locale,
        supportedLocales: const [Locale('en'), Locale('zh', 'CN')],
        localizationsDelegates: GlobalMaterialLocalizations.delegates,
        home: Scaffold(
          body: SingleChildScrollView(
            child: ToolRecord(
              host: host,
              session: fixture.session(),
              command: (_, _) async => {},
              enabled: true,
              page: {
                'entries': [
                  {
                    'id': 'call',
                    'parts': [
                      {
                        'kind': 'tool_call',
                        'data': {
                          'arguments': contract['arguments'],
                          'display': contract['display'],
                        },
                      },
                    ],
                  },
                  if (!pending)
                    {
                      'id': 'result',
                      'parts': [
                        {
                          'kind': 'tool_result',
                          'data': {'result': contract['result'], 'images': []},
                        },
                      ],
                    },
                ],
              },
              call: {
                'name': name,
                'turn': 'turn',
                'state': pending ? 'waiting' : 'returned',
                'presentation': 'details',
                'content': object(contract['content']),
                'resolved': resolved,
                'source': {'entry': 'call', 'index': 0},
                if (!pending) 'response': {'entry': 'result', 'index': 0},
                if (pending) 'approval': {'id': 'approval', 'state': 'pending'},
              },
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final label = object(resolved['label']);
    final title = text(
      object(label['locales'])[locale.toLanguageTag()],
      text(label['label'], name),
    );
    final summary = text(object(object(resolved['input'])['summary'])['text']);
    await tester.tap(
      find.text(
        [title, if (summary.isNotEmpty) summary.split('\n').first].join(' '),
      ),
    );
    await tester.pumpAndSettle();
  }

  for (final contract in contracts) {
    final name = text(contract['name']).split('_').last;
    testWidgets('captured $name contract', (tester) async {
      await record(tester, contract);
      expect(find.text(tr('toolRaw')), findsNothing);
      expect(find.text(tr('toolArguments')), findsNothing);
      switch (name) {
        case 'read':
          expect(find.text('Read src/资料.rs'), findsOneWidget);
          expect(find.text('Original body 中文 🙂'), findsOneWidget);
        case 'run':
          expect(find.text('first\nsecond'), findsOneWidget);
          expect(find.text('Partial output'), findsOneWidget);
          expect(find.text('Exit 7'), findsOneWidget);
        case 'query':
          expect(find.byType(DataTable), findsOneWidget);
          for (final cell in [
            '9223372036854775807',
            '1234567890.12345678901234567890',
            '资料 🙂',
            'NULL',
            '007fff',
          ]) {
            expect(find.text(cell), findsOneWidget);
          }
        case 'write':
          expect(find.byType(DiffView), findsOneWidget);
          expect(find.text('New content 中文 🙂'), findsNWidgets(2));
        case 'failure':
          expect(find.text('Original diagnostic\nFull trace'), findsOneWidget);
          expect(find.text('Done'), findsNothing);
      }
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('captured localized approval preserves input', (tester) async {
    await record(
      tester,
      contracts[3],
      locale: const Locale('zh', 'CN'),
      pending: true,
    );
    expect(find.text('替换 new.txt'), findsOneWidget);
    expect(find.text('New content 中文 🙂'), findsOneWidget);
  });

  testWidgets('MCP failure is not a successful status', (tester) async {
    await record(tester, {
      'name': 'fixture_mcp',
      'arguments': {},
      'result': {
        'output': {
          'isError': true,
          'content': [
            {'type': 'text', 'text': 'Original MCP failure'},
          ],
        },
      },
    });
    expect(find.text('Original MCP failure'), findsOneWidget);
    expect(find.text(tr('conversationToolReturned')), findsNothing);
  });

  for (final code in ['cancelled', 'outcome_unknown']) {
    testWidgets('$code retains the original uncertain outcome', (tester) async {
      await record(tester, {
        'name': 'fixture_action',
        'arguments': {},
        'result': {
          'error': {'code': code, 'message': 'Original outcome'},
        },
        'resolved': {
          'status': {'label': 'Done', 'locales': {}},
        },
      });
      expect(find.text('Original outcome'), findsOneWidget);
      expect(find.text('Done'), findsNothing);
      expect(find.text(tr('toolFailed')), findsNothing);
      expect(
        find.text(
          tr(code == 'cancelled' ? 'toolCancelled' : 'toolOutcomeUnknown'),
        ),
        findsOneWidget,
      );
    });
  }

  test('literal content references decode escaped keys', () {
    expect(
      contentValue({
        'escaped/key': {'~value': '资料 🙂'},
      }, '/escaped~1key/~0value'),
      '资料 🙂',
    );
    expect(
      contentValue({
        'values': ['first'],
      }, '/values/0'),
      'first',
    );
    expect(
      contentValue({
        'values': ['first'],
      }, '/values/1'),
      isNull,
    );
  });

  test('added diff preserves original text and coordinates', () {
    final lines = diffLines('Original 中文 🙂\nsecond\n', added: true);
    expect(
      lines.map((line) => [line.text, line.before, line.after, line.kind]),
      [
        ['Original 中文 🙂', null, 1, 'added'],
        ['second', null, 2, 'added'],
      ],
    );
    expect(diffLines('', added: true), isEmpty);
  });
  testWidgets('sent file reference remains visible', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: LiveTimeline(
            host: host,
            session: fixture.session(),
            command: (_, _) async => {},
            view: fixture.view(
              entries: [
                {
                  'id': 'user',
                  'turn': 'turn',
                  'author': 'user',
                  'parts': [
                    {'kind': 'text', 'data': 'Review this file'},
                    {
                      'kind': 'reference',
                      'data': {
                        'target': {'kind': 'file', 'data': 'src/main.rs'},
                        'label': 'src/main.rs',
                      },
                    },
                  ],
                },
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('src/main.rs'), findsOneWidget);
  });
  testWidgets('validated tool table is rendered as cells', (tester) async {
    final content = {
      'version': 1,
      'blocks': [
        {
          'kind': 'table',
          'columns': ['Name'],
          'rows': [
            ['Widget'],
          ],
          'truncated': false,
        },
      ],
    };
    final page = {
      'entries': [
        {
          'id': 'call',
          'parts': [
            {
              'kind': 'tool_call',
              'data': {'arguments': {}},
            },
          ],
        },
        {
          'id': 'result',
          'parts': [
            {
              'kind': 'tool_result',
              'data': {
                'result': {'sailry_content': content},
                'images': [],
              },
            },
          ],
        },
      ],
    };
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ToolRecord(
            host: host,
            session: fixture.session(),
            command: (_, _) async => {},
            enabled: true,
            page: page,
            call: {
              'name': 'fixture_query',
              'turn': 'turn',
              'state': 'returned',
              'presentation': 'content',
              'content': content,
              'source': {'entry': 'call', 'index': 0},
              'response': {'entry': 'result', 'index': 0},
            },
          ),
        ),
      ),
    );
    await tester.tap(find.text('fixture_query'));
    await tester.pumpAndSettle();
    expect(find.text('Widget'), findsOneWidget);
  });
  testWidgets('goal results use captured content, not snapshot state', (
    tester,
  ) async {
    final content = {
      'version': 1,
      'blocks': [
        {
          'kind': 'notice',
          'message': {
            'label': 'Completed',
            'locales': {'zh-CN': '已完成'},
          },
        },
        {'kind': 'text', 'path': '/goal/description'},
      ],
    };
    await record(tester, {
      'name': 'fixture_update_goal',
      'arguments': {},
      'resolved': {
        'label': {
          'label': 'Update goal',
          'locales': {'zh-CN': '更新目标'},
        },
      },
      'content': content,
      'result': {
        'goal': {
          'id': 'goal',
          'description': 'Verify the result 中文 🙂',
          'state': 'completed',
          'revision': '2',
        },
        'sailry_content': content,
      },
    }, locale: const Locale('zh', 'CN'));
    expect(find.text('更新目标'), findsOneWidget);
    expect(find.text('已完成'), findsWidgets);
    expect(find.text('Verify the result 中文 🙂'), findsOneWidget);
  });
  testWidgets('unpaired tool response remains inspectable', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: LiveTimeline(
            host: host,
            session: fixture.session(),
            command: (_, _) async => {},
            view: fixture.view(
              entries: [
                {
                  'id': 'result',
                  'turn': 'turn',
                  'author': 'tool',
                  'parts': [
                    {
                      'kind': 'tool_result',
                      'data': {
                        'name': 'fixture_tool',
                        'result': 'Preserved response',
                        'images': [],
                      },
                    },
                  ],
                },
              ],
              calls: [
                {
                  'name': 'fixture_tool',
                  'turn': 'turn',
                  'state': 'returned',
                  'presentation': 'summary',
                  'source': {'entry': 'result', 'index': 0},
                  'response': {'entry': 'result', 'index': 0},
                },
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    expect(find.text('fixture_tool'), findsOneWidget);
    await tester.tap(find.text('fixture_tool'));
    await tester.pumpAndSettle();
    expect(find.text('Preserved response'), findsOneWidget);
  });
}
