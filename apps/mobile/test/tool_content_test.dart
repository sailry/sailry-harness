import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/features/conversations/live/tool_content.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  testWidgets('failed captured content preserves diagnostics once', (
    tester,
  ) async {
    final content = {
      'version': 1,
      'blocks': [
        {'kind': 'text', 'path': '/content/0/text'},
        {
          'kind': 'notice',
          'message': {'label': 'Done', 'locales': {}},
        },
      ],
    };
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ToolContent(
            call: {'name': 'fixture_action', 'content': content},
            result: {
              'result': {
                'output': {
                  'isError': true,
                  'content': [
                    {'type': 'text', 'text': 'Original diagnostic'},
                  ],
                  'sailry_content': content,
                },
              },
            },
            arguments: const {},
            host: HostConnection.test(
              id: 'node',
              label: 'Node',
              command: (_, _) async => {},
            ),
            worktree: 'tree',
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Original diagnostic'), findsOneWidget);
    expect(find.text('Done'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final wrapped in [false, true]) {
    for (final noticeOnly in [false, true]) {
      testWidgets(
        'failed ${wrapped ? 'wrapped' : 'direct'} literal ${noticeOnly ? 'notices' : 'output'} remains readable',
        (tester) async {
          String? copied;
          final messenger =
              TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
          messenger.setMockMethodCallHandler(SystemChannels.platform, (
            call,
          ) async {
            if (call.method == 'Clipboard.setData') {
              copied = call.arguments['text'] as String;
            }
            return null;
          });
          addTearDown(
            () => messenger.setMockMethodCallHandler(
              SystemChannels.platform,
              null,
            ),
          );
          final literal = noticeOnly ? '' : '\t<diagnostic>\nSecond line\n';
          final content = {
            'version': 1,
            'blocks': [
              {
                'kind': 'text',
                'path': '/stream',
                'notices': [
                  {
                    'label': 'Partial fallback',
                    'locales': {'en': 'Partial result'},
                  },
                ],
              },
              if (!noticeOnly)
                {
                  'kind': 'notice',
                  'message': {
                    'label': 'Failure fallback',
                    'locales': {'en': 'Request unavailable'},
                  },
                },
            ],
          };
          final payload = {
            'isError': true,
            'stream': literal,
            'sailry_content': content,
          };
          await tester.pumpWidget(
            MaterialApp(
              home: Scaffold(
                body: ToolContent(
                  call: {'name': 'fixture_action', 'content': content},
                  result: {
                    'result': wrapped ? {'output': payload} : payload,
                  },
                  arguments: const {},
                  host: HostConnection.test(
                    id: 'node',
                    label: 'Node',
                    command: (_, _) async => {},
                  ),
                  worktree: 'tree',
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          expect(find.text('Partial result'), findsOneWidget);
          expect(find.text('Partial fallback'), findsNothing);
          if (noticeOnly) {
            expect(find.byType(CodeText), findsNothing);
          } else {
            expect(find.text('Request unavailable'), findsOneWidget);
            expect(
              tester.widget<CodeText>(find.byType(CodeText)).text,
              literal,
            );
          }
          await tester.tap(find.byType(CopyTextButton).last);
          await tester.pumpAndSettle();
          expect(copied, noticeOnly ? 'Partial result' : literal);
          await tester.pump(const Duration(seconds: 3));
          await tester.pumpAndSettle();
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  for (final table in [true, false]) {
    testWidgets('copies complete ${table ? 'table' : 'notice'} result', (
      tester,
    ) async {
      String? copied;
      final messenger =
          TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
      messenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
        if (call.method == 'Clipboard.setData') {
          copied = call.arguments['text'] as String;
        }
        return null;
      });
      addTearDown(
        () => messenger.setMockMethodCallHandler(SystemChannels.platform, null),
      );
      final content = {
        'version': 1,
        'blocks': [
          if (table)
            {
              'kind': 'table',
              'columns': ['Value'],
              'rows': [
                ['001'],
                ['1.20'],
              ],
              'truncated': false,
            }
          else
            {
              'kind': 'notice',
              'message': {'label': 'Completed', 'locales': {}},
            },
        ],
      };
      final result = {'sailry_content': content};
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ToolContent(
              call: {'name': 'fixture_action', 'content': content},
              result: {'result': result},
              arguments: const {},
              host: HostConnection.test(
                id: 'node',
                label: 'Node',
                command: (_, _) async => {},
              ),
              worktree: 'tree',
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('copy')));
      await tester.pumpAndSettle();
      expect(
        copied,
        table
            ? const JsonEncoder.withIndent('  ').convert(result)
            : 'Completed',
      );
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('command outcomes remain visible without stderr or output', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final theme = ThemeData.dark();
    for (final (outcome, label, failed) in [
      (
        {'kind': 'exited', 'data': 0},
        tr('toolExitCode').replaceAll('{code}', '0'),
        false,
      ),
      (
        {'kind': 'exited', 'data': 1},
        tr('toolExitCode').replaceAll('{code}', '1'),
        false,
      ),
      (
        {'kind': 'signal', 'data': 9},
        tr('toolSignal').replaceAll('{signal}', '9'),
        true,
      ),
      ({'kind': 'timed_out'}, tr('toolTimedOut'), true),
      ({'kind': 'cancelled'}, tr('toolCancelled'), false),
      (
        {'kind': 'unknown', 'data': 'wait failed'},
        tr('toolOutcomeUnknown'),
        false,
      ),
    ]) {
      await tester.pumpWidget(
        MaterialApp(
          theme: theme,
          home: Scaffold(
            body: ToolContent(
              call: const {'name': 'run_command'},
              result: {
                'result': {
                  'kind': 'command_result',
                  'data': {
                    'outcome': outcome,
                    'stdout': {'text': ''},
                    'stderr': {'text': ''},
                  },
                },
              },
              arguments: const {},
              host: host,
              worktree: 'tree',
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text(label), findsOneWidget);
      expect(
        tester.widget<Text>(find.text(label)).style?.color,
        failed ? theme.colorScheme.error : theme.colorScheme.onSurfaceVariant,
      );
      if (outcome['kind'] == 'unknown') {
        expect(find.text('wait failed'), findsOneWidget);
      }
      expect(tester.takeException(), isNull);
    }
  });

  testWidgets('command output uses the subdued text tone', (tester) async {
    final theme = ThemeData.dark();
    await tester.pumpWidget(
      MaterialApp(
        theme: theme,
        home: Scaffold(
          body: ToolContent(
            call: const {'name': 'run_command'},
            result: const {
              'result': {
                'kind': 'command_result',
                'data': {
                  'outcome': {'kind': 'exited', 'data': 1},
                  'stdout': {'text': 'matched file'},
                  'stderr': {'text': ''},
                },
              },
            },
            arguments: const {},
            host: HostConnection.test(
              id: 'node',
              label: 'Node',
              command: (_, _) async => {},
            ),
            worktree: 'tree',
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('matched file'), findsOneWidget);
    expect(
      tester.widget<CodeText>(find.byType(CodeText)).style?.color,
      theme.colorScheme.onSurfaceVariant,
    );
    expect(
      find.text(tr('toolExitCode').replaceAll('{code}', '1')),
      findsOneWidget,
    );
  });
}
