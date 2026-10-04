import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> response(String message) => {
  'data': {
    'page': {
      'entries': [
        {
          'parts': [
            {'kind': 'text', 'data': message},
          ],
        },
        {
          'parts': [
            {
              'kind': 'tool_result',
              'data': {'result': 'private output'},
            },
          ],
        },
      ],
    },
    'missing': [],
  },
};

void main() {
  testWidgets('finds the last message across tool pages', (tester) async {
    final commands = <String>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) async {
        commands.add(kind);
        if (kind == 'read_conversation') {
          return {
            'data': {
              'page': {
                'revision': 7,
                'entries': [
                  {
                    'sequence': 200,
                    'turn': 'turn',
                    'parts': [
                      {'kind': 'tool_result', 'data': {}},
                    ],
                  },
                ],
              },
              'missing': ['turn'],
            },
          };
        }
        expect(kind, 'read_turn');
        expect(data!['expected_revision'], 7);
        expect(data['before'], 200);
        return {
          'data': {
            'entries': [
              {
                'sequence': 199,
                'turn': 'turn',
                'parts': [
                  {'kind': 'text', 'data': 'Last actual message'},
                ],
              },
            ],
            'next_before': 199,
          },
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ConversationTask(
            host: host,
            session: const {
              'id': 'session',
              'activity': {'title': 'Task'},
            },
            project: const {},
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Last actual message'), findsOneWidget);
    expect(commands, ['read_conversation', 'read_turn']);
    await tester.pumpWidget(const SizedBox());
    await host.close();
  });

  test('message text excludes tools and thinking', () {
    expect(
      messagePreview([
        {
          'parts': [
            {'kind': 'text', 'data': 'Previous'},
          ],
        },
        {
          'parts': [
            {'kind': 'text', 'data': 'Latest\nmessage 🙂'},
          ],
        },
        {
          'parts': [
            {'kind': 'thinking', 'data': 'Reasoning'},
          ],
        },
        {
          'parts': [
            {
              'kind': 'tool_result',
              'data': {'result': 'output'},
            },
          ],
        },
      ]),
      'Latest message 🙂',
    );
    expect(
      messagePreview([
        {
          'parts': [
            {
              'kind': 'attachment',
              'data': {
                'spec': {'name': 'photo.png'},
              },
            },
          ],
        },
      ]),
      'photo.png',
    );
  });

  testWidgets('activity refresh ignores stale hosts', (tester) async {
    final replies = <Completer<Map<String, dynamic>>>[];
    final reads = <Map<String, dynamic>>[];
    HostConnection host(String id) => HostConnection.test(
      id: id,
      label: id,
      command: (kind, data) {
        expect(kind, 'read_conversation');
        expect(data!['limit'], 1);
        reads.add(data);
        final reply = Completer<Map<String, dynamic>>();
        replies.add(reply);
        return reply.future;
      },
    );
    final first = host('first');
    final second = host('second');
    var current = first;
    var revision = 1;
    Future<void> mount() => tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: Scaffold(
          body: SizedBox(
            width: 300,
            child: ConversationTask(
              host: current,
              session: {
                'id': 'session',
                'revision': revision,
                'activity': {
                  'title':
                      'A long conversation title that must remain on one line',
                  'run': {'status': 'completed'},
                },
              },
              project: const {
                'appearance': {'icon': 'ai', 'color': 'blue'},
              },
            ),
          ),
        ),
      ),
    );
    await mount();
    replies.single.complete(response('Latest\nmessage'));
    await tester.pumpAndSettle();
    expect(find.text('Latest message'), findsOneWidget);
    final preview = tester.widget<Text>(find.text('Latest message'));
    expect(preview.maxLines, 1);
    expect(preview.overflow, TextOverflow.ellipsis);
    await mount();
    expect(reads.length, 1);
    revision++;
    await mount();
    expect(reads.length, 2);
    current = second;
    await mount();
    expect(reads.length, 3);
    replies[2].complete(response('Second host message'));
    await tester.pumpAndSettle();
    replies[1].complete(response('Stale first host message'));
    await tester.pumpAndSettle();
    expect(find.text('Second host message'), findsOneWidget);
    expect(find.textContaining('Stale'), findsNothing);
    current.connected = false;
    await mount();
    expect(reads.length, 3);
    expect(find.text('Second host message'), findsOneWidget);
    expect(find.byTooltip(tr('offline')), findsOneWidget);
    current.connected = true;
    await mount();
    replies.last.complete(response('After reconnect'));
    await tester.pumpAndSettle();
    expect(find.text('After reconnect'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await first.close();
    await second.close();
  });
}
