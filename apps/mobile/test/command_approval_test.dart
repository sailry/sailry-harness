import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_record.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  testWidgets('description stays visible beside the pending command', (
    tester,
  ) async {
    const command = 'printf checked > result.txt';
    const description = 'Check the writable project directory';
    final requests = <Map<String, dynamic>>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ToolRecord(
            call: const {
              'name': 'run_command',
              'turn': 'turn',
              'source': {'entry': 'entry', 'index': 0},
              'state': 'waiting',
              'approval': {'id': 'approval', 'state': 'pending'},
              'resolved': {
                'label': {'label': 'Command'},
                'input': {
                  'summary': {'text': command, 'code': true},
                  'content': {'text': command, 'code': true},
                },
                'approval': [
                  {'label': command},
                  {'label': description},
                ],
              },
            },
            page: const {
              'entries': [
                {
                  'id': 'entry',
                  'parts': [
                    {
                      'kind': 'tool_call',
                      'data': {
                        'arguments': {
                          'command': command,
                          'description': description,
                        },
                      },
                    },
                  ],
                },
              ],
            },
            session: const {'id': 'session', 'worktree': 'tree'},
            host: HostConnection.test(
              id: 'node',
              label: 'Node',
              command: (_, _) async => {},
            ),
            command: (kind, arguments) async {
              requests.add({'kind': kind, ...arguments});
              return {};
            },
            enabled: true,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text(description), findsOneWidget);
    expect(requests, isEmpty);
    await tester.tap(find.text(tr('allowShort')));
    await tester.pumpAndSettle();
    expect(requests, [
      {
        'kind': 'resolve_approval',
        'session': 'session',
        'approval': 'approval',
        'decision': 'approve',
      },
    ]);
    expect(tester.takeException(), isNull);
  });
}
