import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/commands.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/transcript_index.dart';
import 'package:sailry_mobile/features/conversations/live/tool_row.dart';
import 'package:sailry_mobile/features/conversations/user_message.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

const _commandTitle = 'Run export';
const _commandInput = 'Export report';
const _task = 'command:export';
const _child = {'index': 0, 'target': 'child-target'};
const _commandFailure = 'Report could not be written';
const _childFailure = 'Child could not read a source';
const _hiddenTitles = [
  'Callback on user turn',
  'Historical callback',
  'Automatic callback',
];

Map<String, dynamic> _text(String value) => {'kind': 'text', 'data': value};

Map<String, dynamic> _notice(String id, String title, String outcome) => {
  'id': 'command:$id',
  'kind': {
    'kind': 'command',
    'data': {'id': id},
  },
  'title': title,
  'outcome': outcome,
};

Map<String, dynamic> _row(
  String entry,
  int index,
  Map<String, dynamic> part, {
  bool work = false,
}) => {
  'source': {'entry': entry, 'index': index},
  'part': part,
  'work': work,
  'text_offset': 0,
  'text_last': true,
};

Map<String, dynamic> _projection({
  String command = 'running',
  String child = 'running',
  bool historyOnly = false,
}) {
  final launch = {
    'kind': 'tool_call',
    'data': {'name': 'run_command'},
  };
  final delegate = {
    'kind': 'tool_call',
    'data': {
      'name': 'spawn_agent',
      'summary': {'title': 'Inspect sources'},
    },
  };
  final answer = _text('I started the export.');
  final reply = _text('The earlier callback got an assistant reply.');
  final settled = command != 'running';
  final notice = {
    ..._notice(
      settled ? 'export' : 'earlier',
      _hiddenTitles[1],
      settled ? command : 'completed',
    ),
    if (command == 'failed') 'summary': _commandFailure,
  };
  return {
    'connected': true,
    'snapshot': {
      'page': {
        'session': 'parent',
        'runs': [
          {
            'turn': 'launch',
            'kind': 'task',
            'status': 'completed',
            'notices': [_notice('other', _hiddenTitles[0], 'completed')],
          },
          {
            'turn': 'historical',
            'kind': 'task',
            'status': 'completed',
            'notice': true,
            'notices': [notice],
          },
          {
            'turn': 'automatic',
            'kind': 'task',
            'status': 'queued',
            'notice': true,
            'notices': [_notice('automatic', _hiddenTitles[2], 'completed')],
          },
        ],
        'entries': [
          {
            'id': 'calls',
            'turn': 'launch',
            'author': 'assistant',
            'parts': [launch, delegate],
          },
          {
            'id': 'result',
            'turn': 'launch',
            'author': 'tool',
            'parts': [
              {
                'kind': 'tool_result',
                'data': {
                  'name': 'run_command',
                  'summary': {'task': _task},
                },
              },
            ],
          },
          {
            'id': 'answer',
            'turn': 'launch',
            'author': 'assistant',
            'parts': [answer],
          },
          {
            'id': 'reply',
            'turn': 'historical',
            'author': 'assistant',
            'parts': [reply],
          },
        ],
        'background': [
          if (!historyOnly)
            {
              if (settled)
                ...notice
              else
                ..._notice('export', _commandInput, 'running'),
              'turn': 'launch',
              'state': settled ? 'delivered' : 'running',
              'outcome': settled ? command : null,
              if (settled) 'exit_code': command == 'failed' ? 7 : 0,
            },
        ],
        'children': [
          {
            'origin': {'turn': 'launch', 'entry': 'calls', 'index': 1},
            'run': {
              'session': 'child',
              'turn': 'child-turn',
              'status': child,
              if (child == 'failed') 'error': {'message': _childFailure},
            },
          },
        ],
      },
    },
    'timeline': ['launch', 'historical', 'automatic'],
    'segments': {
      'launch': [
        {
          'inputs': [
            {
              'turn': 'launch',
              'message': {'text': 'Please export the report.'},
              'canonical': true,
            },
          ],
          'rows': [
            _row('calls', 0, launch, work: true),
            _row('calls', 1, delegate, work: true),
            _row('answer', 0, answer),
          ],
        },
      ],
      'historical': [
        {
          'rows': [_row('reply', 0, reply)],
        },
      ],
    },
    'calls': [
      {
        'name': 'run_command',
        'turn': 'launch',
        'source': {'entry': 'calls', 'index': 0},
        'response': {'entry': 'result', 'index': 0},
        'state': 'returned',
        'row': {
          'label': {'label': _commandTitle},
          'detail': {'text': _commandInput},
        },
      },
      {
        'name': 'spawn_agent',
        'turn': 'launch',
        'source': {'entry': 'calls', 'index': 1},
        'state': 'returned',
        'children': [_child],
      },
    ],
    'children': {
      'sessions': {'child': _child},
    },
  };
}

class _CommandConnection extends Fake implements Connection {
  final references = <Object?>[];

  @override
  Future<CommandDetail> openCommandDetail({required String reference}) {
    references.add(jsonDecode(reference));
    return Future.error(StateError('The transcript reads no command output'));
  }
}

void main() {
  late HostConnection host;
  late ScrollController scroll;
  late List<Map<String, dynamic>> opened;
  setUp(() {
    host = HostConnection.test(
      id: 'node',
      label: 'Test host',
      snapshot: {
        'sessions': [
          {'id': 'child'},
        ],
      },
      childTargets: {'child-target': 'child'},
      command: (kind, _) async => fail('Unexpected command: $kind'),
    );
    scroll = ScrollController();
    addTearDown(host.dispose);
    addTearDown(scroll.dispose);
    opened = [];
  });

  Future<void> show(
    WidgetTester tester,
    Map<String, dynamic> view, {
    HostConnection? detailHost,
  }) => tester.pumpWidget(
    MaterialApp(
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(disableAnimations: true),
        child: child!,
      ),
      home: Scaffold(
        body: LiveTimeline(
          transcript: TranscriptIndex(view),
          connected: view['connected'] == true,
          controller: scroll,
          padding: EdgeInsets.zero,
          session: const {'id': 'parent'},
          host: detailHost ?? host,
          command: (kind, _) async => fail('Unexpected command: $kind'),
          onOpenChild: opened.add,
        ),
      ),
    ),
  );

  void expectNoCallbackBubbles() {
    for (final title in _hiddenTitles) {
      expect(find.text(title), findsNothing);
    }
    expect(find.byType(UserBubble), findsOneWidget);
  }

  final command = find.widgetWithText(ToolRow, _commandInput);
  final child = find.byKey(const ValueKey('child-child'));
  ToolRow row(WidgetTester tester, Finder finder) => tester.widget(finder);
  Finder markdown(String text) => find.byWidgetPredicate(
    (widget) => widget is MarkdownContent && widget.data == text,
  );
  Future<void> expand(WidgetTester tester) async {
    await tester.tap(find.byKey(const ValueKey('work-launch')));
    await tester.pumpAndSettle();
  }

  testWidgets(
    'callback bubbles disappear without hiding input or turn status',
    (tester) async {
      await show(tester, _projection());
      expectNoCallbackBubbles();
      expect(find.text('Please export the report.'), findsOneWidget);
      expect(markdown('I started the export.'), findsOneWidget);
      expect(
        markdown('The earlier callback got an assistant reply.'),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: find.byKey(const ValueKey('turn-automatic')),
          matching: find.text(tr('conversationQueued')),
        ),
        findsOneWidget,
      );

      await expand(tester);
      expect(row(tester, command).running, isTrue);
      expect(row(tester, command).state, tr('running'));
      expect(row(tester, command).detail, isNull);
      expect(row(tester, command).action, isNull);
      expect(row(tester, command).onTap, isNull);
      expect(find.text(_commandTitle), findsNothing);
      expect(row(tester, child).running, isTrue);

      await show(tester, _projection(command: 'failed', child: 'completed'));
      expectNoCallbackBubbles();
      expect(row(tester, command).running, isFalse);
      expect(row(tester, command).tone, RowTone.failed);
      expect(row(tester, command).state, '${tr('backgroundFailed')} (7)');
      expect(row(tester, command).detail, isNull);
      expect(
        row(tester, child).detail,
        '${tr('backgroundCompleted')} · Inspect sources',
      );
      await tester.tap(command);
      await tester.pumpAndSettle();
      expect(find.byType(CodeBlock), findsNothing);
      expect(find.text(_commandFailure), findsNothing);

      await show(
        tester,
        _projection(command: 'failed', child: 'completed', historyOnly: true),
      );
      expectNoCallbackBubbles();
      expect(row(tester, command).state, tr('backgroundFailed'));
      expect(row(tester, command).detail, isNull);
      await tester.tap(child);
      expect(opened, [_child]);
    },
  );

  testWidgets('completion and a delegated failure stay on their own rows', (
    tester,
  ) async {
    await show(tester, _projection(command: 'completed', child: 'failed'));
    await expand(tester);
    expectNoCallbackBubbles();
    expect(row(tester, command).tone, RowTone.normal);
    expect(row(tester, command).state, tr('backgroundCompleted'));
    expect(row(tester, command).detail, isNull);
    expect(row(tester, child).tone, RowTone.failed);
    expect(
      row(tester, child).detail,
      '${tr('backgroundFailed')} · Inspect sources',
    );
    await tester.tap(find.text(tr('conversationErrorDetails')));
    await tester.pumpAndSettle();
    expect(find.text(_childFailure), findsOneWidget);
  });

  testWidgets(
    'an old command stays one line and never reads its output',
    (tester) async {
      final connection = _CommandConnection();
      final detailHost = HostConnection.test(
        id: 'node',
        label: 'Test host',
        connection: connection,
      );
      addTearDown(detailHost.dispose);
      final view = _projection(historyOnly: true, child: 'completed');
      final page = (view['snapshot'] as Map)['page'] as Map;
      page['background'] = [
        for (var index = 1; index <= 20; index++)
          {
            ..._notice('new-$index', 'Recent command $index', 'completed'),
            'state': 'consumed',
          },
      ];
      const reference = {
        'session': 'original-execution-session',
        'command': 'export',
        'detail': {
          'session': 'parent',
          'turn': 'launch',
          'entry': 'result',
          'index': 0,
          'call': 'run-export',
          'revision': 'original-result-revision',
        },
      };
      final call = (view['calls'] as List).first as Map;
      call['command_detail'] = reference;
      call['row'] = {
        'label': {'label': _commandTitle},
        'detail': {'text': '\n$_commandInput \\\n  --format csv'},
      };
      await show(tester, view, detailHost: detailHost);
      await expand(tester);
      expectNoCallbackBubbles();
      final line = find.widgetWithText(ToolRow, '$_commandInput \\');
      expect(line, findsOneWidget);
      expect(row(tester, line).detail, isNull);
      expect(row(tester, line).onTap, isNull);
      expect(find.textContaining('--format'), findsNothing);
      expect(find.text(_commandTitle), findsNothing);

      await tester.tap(line);
      await tester.pumpAndSettle();
      expect(connection.references, isEmpty);
      expect(find.byType(CodeBlock), findsNothing);
      expect(find.text(tr('loading')), findsNothing);
      expectNoCallbackBubbles();
    },
  );
}
