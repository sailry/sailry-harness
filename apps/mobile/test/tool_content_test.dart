import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/features/conversations/live/tool_content.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
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
