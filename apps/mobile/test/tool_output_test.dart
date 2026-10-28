import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_output.dart';

void main() {
  Map<String, dynamic> call(String entry) => {
    'source': {'entry': entry, 'index': 0},
  };

  test('combines mounted references and releases only their owner', () {
    final selection = ToolSelection();
    final first = Object();
    final second = Object();
    Map<String, dynamic> update(
      Object owner,
      List<Map<String, dynamic>> calls,
    ) => jsonDecode(selection.update(owner, calls)!) as Map<String, dynamic>;
    expect(update(first, [call('first')])['items'], [
      {'entry': 'first', 'index': 0},
    ]);
    expect(update(second, [call('first'), call('second')])['items'], [
      {'entry': 'first', 'index': 0},
      {'entry': 'second', 'index': 0},
    ]);
    expect(selection.update(first, []), isNull);
    expect(update(second, [])['items'], isEmpty);
  });

  test('unchanged references are silent and retry names only its owner', () {
    final selection = ToolSelection();
    final first = Object();
    final second = Object();
    selection.update(first, [call('first')]);
    selection.update(second, [call('second')]);
    expect(selection.update(first, [call('first')]), isNull);
    final retry =
        jsonDecode(selection.update(first, [call('first')], retry: true)!)
            as Map;
    expect(retry['retry'], [
      {'entry': 'first', 'index': 0},
    ]);
    expect(retry['items'], hasLength(2));
    selection.reset();
    expect(selection.update(first, [call('first')]), isNotNull);
  });

  test('Client state controls retry and summary approval', () {
    for (final limit in ['transfer', 'memory']) {
      expect(
        detailAllowsApproval({
          'detail_state': {'kind': 'too_large', 'limit': limit},
        }),
        isTrue,
      );
    }
    expect(
      detailAllowsApproval({
        'detail_state': {'kind': 'ready'},
      }),
      isTrue,
    );
    for (final kind in ['loading', 'failed', null]) {
      expect(
        detailAllowsApproval({
          'detail_state': {'kind': kind},
        }),
        isFalse,
      );
    }
    expect(
      detailFailed({
        'detail_state': {'kind': 'failed', 'retrying': true},
      }),
      isFalse,
    );
    expect(
      detailFailed({
        'detail_state': {'kind': 'failed', 'retrying': false},
      }),
      isTrue,
    );
    expect(
      toolDetail({
        'detail': {
          'arguments': {'path': 'current.txt'},
        },
      }),
      {
        'arguments': {'path': 'current.txt'},
      },
    );
    expect(toolDetail({'detail': null}), isNull);
  });
  test('missing and contended details permit only rejection', () {
    for (final code in ['busy', 'not_found']) {
      final call = <String, dynamic>{
        'detail_state': {
          'kind': 'failed',
          'fault': {'code': code},
          'retrying': false,
        },
      };
      expect(detailAllowsDecision(call, 'approve'), isFalse);
      expect(detailAllowsDecision(call, 'deny'), isTrue);
      expect(detailFailed(call), isTrue);
    }
    for (final kind in ['loading', null]) {
      final call = <String, dynamic>{
        'detail_state': {'kind': kind},
      };
      expect(detailAllowsDecision(call, 'approve'), isFalse);
      expect(detailAllowsDecision(call, 'deny'), isTrue);
    }
  });
  test('ready deltas reuse only their exact loaded revision', () {
    Map<String, dynamic> view(
      String revision, {
      bool include = false,
      String kind = 'ready',
      String session = 'session',
    }) => {
      'snapshot': {
        'page': {'session': session},
      },
      'calls': [
        <String, dynamic>{
          'id': 'call',
          'turn': 'turn',
          'source': {'entry': 'entry', 'index': 0},
          'revision': revision,
          'detail_key': '$session/$revision',
          'detail_state': {'kind': kind},
          if (include) 'detail': {'result': 'Complete output'},
        },
      ],
    };
    final first = view('one', include: true);
    final detail = (first['calls'] as List)[0]['detail'];
    final merged = mergeToolDetails(first, view('one'));
    expect(identical((merged['calls'] as List)[0]['detail'], detail), isTrue);
    expect(
      (mergeToolDetails(first, view('two'))['calls'] as List)[0]['detail'],
      isNull,
    );
    expect(
      (mergeToolDetails(first, view('one', kind: 'loading'))['calls']
          as List)[0]['detail'],
      isNull,
    );
    expect(
      (mergeToolDetails(first, view('one', session: 'other'))['calls']
          as List)[0]['detail'],
      isNull,
    );
    final otherReference = view('one');
    (otherReference['calls'] as List)[0]['detail_key'] = 'other reference';
    expect(
      (mergeToolDetails(first, otherReference)['calls'] as List)[0]['detail'],
      isNull,
    );
    final released = view('one');
    (released['calls'] as List)[0]['detail'] = null;
    expect(
      (mergeToolDetails(first, released)['calls'] as List)[0]['detail'],
      isNull,
    );
  });
  test('ready deltas preserve current approval facts', () {
    final detail = <String, dynamic>{
      'arguments': {'command': 'Exact command'},
      'approval_view': {'unrestricted': false},
    };
    Map<String, dynamic> view(bool unrestricted, {bool include = false}) => {
      'snapshot': {
        'page': {'session': 'session'},
      },
      'calls': [
        <String, dynamic>{
          'id': 'call',
          'turn': 'turn',
          'source': {'entry': 'entry', 'index': 0},
          'revision': 'same',
          'detail_key': 'same',
          'detail_state': {'kind': 'ready'},
          'approval_view': {'unrestricted': unrestricted},
          if (include) 'detail': detail,
        },
      ],
    };
    final merged = mergeToolDetails(view(false, include: true), view(true));
    final call = (merged['calls'] as List).single as Map;
    expect(identical(call['detail'], detail), isTrue);
    expect(call['approval_view'], {'unrestricted': true});
  });
}
