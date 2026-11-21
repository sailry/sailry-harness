import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';
import 'package:sailry_mobile/features/conversations/live/tool_record.dart';
import 'package:sailry_mobile/features/conversations/live/tool_output.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';
import 'package:sailry_mobile/features/conversations/live/tool_row.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  test('renders Client rows with their citation offsets', () {
    final entry = {
      'id': 'entry',
      'citations': [
        {'uri': 'https://example.com/source', 'end': 3},
      ],
    };
    final draft = {'id': 'draft'};
    final selected = [
      {
        'source': {'entry': 'entry', 'index': 2},
        'draft': false,
        'part': {'kind': 'text', 'data': 'B'},
        'text_offset': 2,
        'text_last': true,
        'work': false,
      },
      {
        'source': {'entry': 'draft', 'index': 0},
        'draft': true,
        'part': {'kind': 'thinking', 'data': 'Working'},
        'work': true,
      },
    ];
    final before = jsonEncode([entry, draft, selected]);
    final rows = selectedRows(
      selected,
      entries: {'entry': entry},
      drafts: {'draft': draft},
    );
    expect(rows.first.$1['id'], 'entry');
    expect(rows.first.$2, 2);
    expect(rows.first.$3['data'], 'B [1](<https://example.com/source>)');
    expect(rows.first.$3['work'], isFalse);
    expect(rows.last.$1['id'], 'draft');
    expect(rows.last.$3['data'], 'Working');
    expect(rows.last.$3['work'], isTrue);
    expect(selectedRows(null, entries: {}, drafts: {}), isEmpty);
    expect(jsonEncode([entry, draft, selected]), before);
  });

  test('resolves source ranges with citations and keeps inline overrides', () {
    const result = {'name': 'capture'};
    const image = {'index': 2};
    final entry = <String, dynamic>{
      'id': 'answer',
      'parts': [
        {'kind': 'text', 'data': ' \n'},
        {'kind': 'thinking', 'data': 'Working '},
        {'kind': 'thinking', 'data': '🙂'},
        {'kind': 'text', 'data': 'A😀'},
        {'kind': 'text', 'data': '中'},
        {'kind': 'tool_result', 'data': result},
        {'kind': 'text', 'data': 'B'},
      ],
      'citations': [
        {'uri': 'https://example.com/merged', 'end': 4},
        {'uri': 'https://example.com/last', 'end': 6},
        {'uri': 'https://example.com/whole'},
      ],
    };
    final draft = <String, dynamic>{
      'id': 'draft',
      'parts': [
        {'kind': 'text', 'data': 'Streaming draft'},
      ],
    };
    Map<String, dynamic> row(
      int start,
      int end, {
      int offset = 0,
      bool last = false,
      bool work = false,
      bool inDraft = false,
    }) => {
      'source': {'entry': inDraft ? 'draft' : 'answer', 'index': start},
      'draft': inDraft,
      'end': end,
      'work': work,
      'text_offset': offset,
      'text_last': last,
    };
    final selected = [
      row(1, 3, offset: 2, work: true),
      row(3, 5, offset: 2),
      row(5, 6, offset: 5)..['part'] = {'kind': 'image', 'data': image},
      row(5, 6, offset: 5, work: true),
      row(6, 7, offset: 5, last: true),
      row(0, 1, inDraft: true, last: true),
    ];
    final before = jsonEncode([entry, draft, selected]);
    final rows = selectedRows(
      selected,
      entries: {'answer': entry},
      drafts: {'draft': draft},
    );
    expect(rows.take(5).every((row) => identical(row.$1, entry)), isTrue);
    expect(rows.last.$1, same(draft));
    expect(
      rows.map(
        (row) => [row.$2, row.$3['kind'], row.$3['data'], row.$3['work']],
      ),
      [
        [1, 'thinking', 'Working 🙂', true],
        [3, 'text', 'A😀 [1](<https://example.com/merged>)中', false],
        [5, 'image', image, false],
        [5, 'tool_result', result, true],
        [
          6,
          'text',
          'B [2](<https://example.com/last>) [3](<https://example.com/whole>)',
          false,
        ],
        [0, 'text', 'Streaming draft', false],
      ],
    );
    expect(jsonEncode([entry, draft, selected]), before);
  });

  test('business fields never establish execution failure', () {
    final call = {
      'row': {'failed': false},
      'failure': null,
      'result': {
        'isError': false,
        'structuredContent': {'isError': true, 'error': 'domain row'},
      },
      'resolved': {
        'diagnostics': [
          {'text': 'Declared diagnostic', 'error': true},
        ],
        'command': {'kind': 'signal', 'data': 9},
      },
    };
    final before = jsonEncode(call);
    expect(toolFailed(call), isFalse);
    expect(jsonEncode(call), before);
    expect(
      toolFailed({
        'row': {'failed': true},
        'failure': <String, dynamic>{},
      }),
      isTrue,
    );
    for (final code in ['cancelled', 'outcome_unknown']) {
      expect(
        toolFailed({
          'row': {'failed': false},
          'failure': {'code': code},
        }),
        isFalse,
      );
    }
  });

  testWidgets('tool errors keep one short line and never load the result', (
    tester,
  ) async {
    const message = 'The reviewed path is missing';
    const path = 'src/review.dart';
    const raw = {'provider_reply': 'original result'};
    final selections = <List<Map<String, dynamic>>>[];
    Future<void> show({
      String? error,
      bool loaded = false,
      String name = 'fixture_tool',
      Map<String, dynamic> row = const {},
    }) => tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ToolRecord(
            call: {
              'name': name,
              'state': 'returned',
              'source': {'entry': 'call', 'index': 0},
              'response': {'entry': 'result', 'index': 0},
              'failure': {'origin': 'tool'},
              'error': ?error,
              if (error == null) 'error_detail': true,
              'row': {
                'warning': true,
                'detail': {'text': path},
                ...row,
              },
              if (loaded) 'detail': {'result': raw},
            },
            page: const {},
            session: const {'id': 'session'},
            command: (kind, _) async => fail('Unexpected command: $kind'),
            enabled: true,
            toolOutput: (_, calls, _) => selections.add(calls),
          ),
        ),
      ),
    );
    final rawText = const JsonEncoder.withIndent('  ').convert(raw);

    await show(error: '$message\nwith a second line', loaded: true);
    expect(find.text(message), findsOneWidget);
    expect(find.textContaining('second line'), findsNothing);
    expect(tester.widget<ToolRow>(find.byType(ToolRow)).detail, path);
    expect(find.text(rawText), findsNothing);
    expect(selections, isEmpty);

    await show();
    expect(selections, isEmpty);
    expect(find.text(tr('toolFailed')), findsOneWidget);
    expect(find.text(rawText), findsNothing);

    await show(loaded: true);
    expect(find.text(rawText), findsNothing);
    expect(selections, isEmpty);

    // A denied call reads as its row state alone.
    await show(row: {'state': 'denied'});
    expect(
      tester.widget<ToolRow>(find.byType(ToolRow)).state,
      tr('failureDenied'),
    );
    expect(find.text(tr('failureDenied')), findsOneWidget);

    // A command is its command line and a short state, without a message.
    await show(error: 'exit status 2', name: 'run_command');
    final command = tester.widget<ToolRow>(find.byType(ToolRow));
    expect(command.label, path);
    expect(command.detail, isNull);
    expect(command.state, tr('toolFailed'));
    expect(command.onTap, isNull);
    expect(find.text('exit status 2'), findsNothing);
  });

  test("reads captured approval arguments and summarized question inputs", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "approvals retain their loaded arguments across deltas",
        (caseName) {
          final arguments = <String, dynamic>{'command': 'echo exact'};
          final call = <String, dynamic>{
            'source': {'entry': 'entry', 'index': 0},
            'detail_key': 'approval/revision',
            'detail_state': {'kind': 'ready'},
            'approval_view': {'unrestricted': false},
            'detail': {'arguments': arguments, 'result': 'Unneeded output'},
          };
          Map<String, dynamic> view(Map<String, dynamic> call) => {
            'snapshot': {
              'page': {'session': 'session'},
            },
            'calls': [call],
          };
          final next = <String, dynamic>{
            'source': call['source'],
            'detail_key': call['detail_key'],
            'detail_state': call['detail_state'],
            'approval_view': {'unrestricted': true},
          };
          final merged = mergeToolDetails(view(call), view(next));
          final current =
              (merged['calls'] as List).single as Map<String, dynamic>;
          final before = jsonEncode(current);
          expect(current['detail'], same(call['detail']), reason: caseName);
          expect(
            toolArguments(current, const {}),
            equals(arguments),
            reason: caseName,
          );
          expect(current['approval_view'], {
            'unrestricted': true,
          }, reason: caseName);
          expect(
            detailAllowsDecision(current, 'approve'),
            isTrue,
            reason: caseName,
          );
          expect(jsonEncode(current), before, reason: caseName);
        },
      ),
      (
        "questions use their summary without loading output",
        (caseName) {
          final spec = <String, dynamic>{
            'prompt': 'Choose an option',
            'input': {
              'kind': 'choice',
              'options': ['First', 'Second'],
              'multiple': false,
              'allow_other': true,
            },
          };
          final page = <String, dynamic>{
            'entries': [
              {
                'id': 'entry',
                'parts': [
                  {'kind': 'text', 'data': 'Before'},
                  {
                    'kind': 'tool_call',
                    'data': {
                      'summary': {'question': spec},
                    },
                  },
                ],
              },
            ],
          };
          final call = <String, dynamic>{
            'source': {'entry': 'entry', 'index': 1},
            'question': {
              'state': {'kind': 'pending'},
            },
            'details': false,
          };
          final before = jsonEncode([call, page]);
          expect(toolArguments(call, page), equals(spec), reason: caseName);
          expect(toolDetail(call), isNull, reason: caseName);
          expect(jsonEncode([call, page]), before, reason: caseName);
          for (final source in [
            {'entry': 'missing', 'index': 1},
            {'entry': 'entry', 'index': -1},
            {'entry': 'entry', 'index': 2},
          ]) {
            expect(
              toolArguments({'source': source}, page),
              isEmpty,
              reason: caseName,
            );
          }
        },
      ),
    ]) {
      run(caseName);
    }
  });

  // The suite has no widget harness; the card is the only approval surface,
  // so its source must hold no serializer for a call's arguments.
  test('an approval card never prints a tool\'s raw arguments', () {
    final source = File(
      'lib/features/conversations/live/tool_record.dart',
    ).readAsStringSync();
    expect(source, isNot(contains('dart:convert')));
    expect(source, isNot(contains('JsonEncoder')));
    expect(source, isNot(contains('jsonEncode')));
  });
}
