import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/message_data.dart';
import 'package:sailry_mobile/features/conversations/live/tool_record.dart';
import 'package:sailry_mobile/features/conversations/live/tool_output.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';

void main() {
  test('renders Client rows with their citation offsets', () {
    final view = {
      'snapshot': {
        'page': {
          'entries': [
            {
              'id': 'entry',
              'citations': [
                {'uri': 'https://example.com/source', 'end': 3},
              ],
            },
          ],
        },
        'drafts': [
          {'id': 'draft'},
        ],
      },
      'rows': {
        'turn': [
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
        ],
      },
    };
    final before = jsonEncode(view);
    final rows = displayRows(view, 'turn');
    expect(rows.first.$1['id'], 'entry');
    expect(rows.first.$2, 2);
    expect(rows.first.$3['data'], 'B [1](<https://example.com/source>)');
    expect(rows.first.$3['work'], isFalse);
    expect(rows.last.$1['id'], 'draft');
    expect(rows.last.$3['data'], 'Working');
    expect(rows.last.$3['work'], isTrue);
    expect(displayRows(view, 'other'), isEmpty);
    expect(jsonEncode(view), before);
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

  test('approvals retain their loaded arguments across deltas', () {
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
    final current = (merged['calls'] as List).single as Map<String, dynamic>;
    final before = jsonEncode(current);
    expect(current['detail'], same(call['detail']));
    expect(toolArguments(current, const {}), equals(arguments));
    expect(current['approval_view'], {'unrestricted': true});
    expect(detailAllowsDecision(current, 'approve'), isTrue);
    expect(jsonEncode(current), before);
  });

  test('questions use their summary without loading output', () {
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
    expect(toolArguments(call, page), equals(spec));
    expect(toolDetail(call), isNull);
    expect(jsonEncode([call, page]), before);
    for (final source in [
      {'entry': 'missing', 'index': 1},
      {'entry': 'entry', 'index': -1},
      {'entry': 'entry', 'index': 2},
    ]) {
      expect(toolArguments({'source': source}, page), isEmpty);
    }
  });
}
