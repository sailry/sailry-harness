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
}
