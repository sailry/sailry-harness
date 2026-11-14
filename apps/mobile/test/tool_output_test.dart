import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_output.dart';

void main() {
  test('detail failures notify once until cleared or changed', () {
    const failure = {'code': 'unavailable', 'message': 'Detail unavailable'};
    const changed = {'code': 'not_found', 'message': 'Detail removed'};
    final failed = {
      'detail_errors': [failure, failure],
    };
    expect(newDetailErrors({}, failed), [failure]);
    expect(newDetailErrors(failed, failed), isEmpty);
    expect(
      newDetailErrors(failed, {
        'detail_errors': [failure, changed],
      }),
      [changed],
    );
    expect(newDetailErrors(failed, {}), isEmpty);
    expect(newDetailErrors({}, failed), [failure]);
  });

  Map<String, dynamic> call(String entry) => {
    'source': {'entry': entry, 'index': 0},
  };

  test("retains only mounted selection and explicit retry owners", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "combines mounted references and releases only their owner",
        (caseName) {
          final selection = ToolSelection();
          final first = Object();
          final second = Object();
          Map<String, dynamic> update(
            Object owner,
            List<Map<String, dynamic>> calls,
          ) =>
              jsonDecode(selection.update(owner, calls)!)
                  as Map<String, dynamic>;
          expect(update(first, [call('first')])['items'], [
            {'entry': 'first', 'index': 0},
          ], reason: caseName);
          expect(update(second, [call('first'), call('second')])['items'], [
            {'entry': 'first', 'index': 0},
            {'entry': 'second', 'index': 0},
          ], reason: caseName);
          expect(selection.update(first, []), isNull, reason: caseName);
          expect(update(second, [])['items'], isEmpty, reason: caseName);
        },
      ),
      (
        "unchanged references are silent and retry names only its owner",
        (caseName) {
          final selection = ToolSelection();
          final first = Object();
          final second = Object();
          selection.update(first, [call('first')]);
          selection.update(second, [call('second')]);
          expect(
            selection.update(first, [call('first')]),
            isNull,
            reason: caseName,
          );
          final retry =
              jsonDecode(selection.update(first, [call('first')], retry: true)!)
                  as Map;
          expect(retry['retry'], [
            {'entry': 'first', 'index': 0},
          ], reason: caseName);
          expect(retry['items'], hasLength(2), reason: caseName);
          selection.reset();
          expect(
            selection.update(first, [call('first')]),
            isNotNull,
            reason: caseName,
          );
        },
      ),
    ]) {
      run(caseName);
    }
  });

  test("uses Client detail readiness for approval and rejection", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "Client state controls retry and summary approval",
        (caseName) {
          for (final limit in ['transfer', 'memory']) {
            expect(
              detailAllowsApproval({
                'detail_state': {'kind': 'too_large', 'limit': limit},
              }),
              isTrue,
              reason: caseName,
            );
          }
          expect(
            detailAllowsApproval({
              'detail_state': {'kind': 'ready'},
            }),
            isTrue,
            reason: caseName,
          );
          for (final kind in ['loading', 'failed', null]) {
            expect(
              detailAllowsApproval({
                'detail_state': {'kind': kind},
              }),
              isFalse,
              reason: caseName,
            );
          }
          expect(
            detailFailed({
              'detail_state': {'kind': 'failed', 'retrying': true},
            }),
            isFalse,
            reason: caseName,
          );
          expect(
            detailFailed({
              'detail_state': {'kind': 'failed', 'retrying': false},
            }),
            isTrue,
            reason: caseName,
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
            reason: caseName,
          );
          expect(toolDetail({'detail': null}), isNull, reason: caseName);
        },
      ),
      (
        "missing and contended details permit only rejection",
        (caseName) {
          for (final code in ['busy', 'not_found']) {
            final call = <String, dynamic>{
              'detail_state': {
                'kind': 'failed',
                'fault': {'code': code},
                'retrying': false,
              },
            };
            expect(
              detailAllowsDecision(call, 'approve'),
              isFalse,
              reason: caseName,
            );
            expect(
              detailAllowsDecision(call, 'deny'),
              isTrue,
              reason: caseName,
            );
            expect(detailFailed(call), isTrue, reason: caseName);
          }
          for (final kind in ['loading', null]) {
            final call = <String, dynamic>{
              'detail_state': {'kind': kind},
            };
            expect(
              detailAllowsDecision(call, 'approve'),
              isFalse,
              reason: caseName,
            );
            expect(
              detailAllowsDecision(call, 'deny'),
              isTrue,
              reason: caseName,
            );
          }
        },
      ),
    ]) {
      run(caseName);
    }
  });

  test("preserves exact loaded revisions and current approval facts", () {
    for (final (caseName, run) in <(String, void Function(String))>[
      (
        "ready deltas reuse only their exact loaded revision",
        (caseName) {
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
          expect(
            identical((merged['calls'] as List)[0]['detail'], detail),
            isTrue,
            reason: caseName,
          );
          expect(
            (mergeToolDetails(first, view('two'))['calls']
                as List)[0]['detail'],
            isNull,
            reason: caseName,
          );
          expect(
            (mergeToolDetails(first, view('one', kind: 'loading'))['calls']
                as List)[0]['detail'],
            isNull,
            reason: caseName,
          );
          expect(
            (mergeToolDetails(first, view('one', session: 'other'))['calls']
                as List)[0]['detail'],
            isNull,
            reason: caseName,
          );
          final otherReference = view('one');
          (otherReference['calls'] as List)[0]['detail_key'] =
              'other reference';
          expect(
            (mergeToolDetails(first, otherReference)['calls']
                as List)[0]['detail'],
            isNull,
            reason: caseName,
          );
          final released = view('one');
          (released['calls'] as List)[0]['detail'] = null;
          expect(
            (mergeToolDetails(first, released)['calls'] as List)[0]['detail'],
            isNull,
            reason: caseName,
          );
        },
      ),
      (
        "ready deltas preserve current approval facts",
        (caseName) {
          final detail = <String, dynamic>{
            'arguments': {'command': 'Exact command'},
            'approval_view': {'unrestricted': false},
          };
          Map<String, dynamic> view(
            bool unrestricted, {
            bool include = false,
          }) => {
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
          final merged = mergeToolDetails(
            view(false, include: true),
            view(true),
          );
          final call = (merged['calls'] as List).single as Map;
          expect(identical(call['detail'], detail), isTrue, reason: caseName);
          expect(call['approval_view'], {
            'unrestricted': true,
          }, reason: caseName);
        },
      ),
    ]) {
      run(caseName);
    }
  });
}
