import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/presentation.dart';
import 'package:sailry_mobile/runtime/session.dart';

String _key(String key) => key;

void main() {
  test('a context overflow has its own failure label', () {
    expect(
      failureLabel(const CommandFailure('context_overflow'), translate: _key),
      'conversationContextOverflow',
    );
    expect(
      failureLabel(const CommandFailure('internal'), translate: _key),
      'conversationFailed',
    );
    expect(
      failureLabel(const CommandFailure('outcome_unknown'), translate: _key),
      'conversationUnknown',
    );
  });

  test('a failed run names a context overflow above its message', () {
    expect(
      runFailure({
        'code': 'context_overflow',
        'message': 'too long',
      }, translate: _key),
      'conversationContextOverflow\ntoo long',
    );
    expect(
      runFailure({'code': 'context_overflow'}, translate: _key),
      'conversationContextOverflow',
    );
    // Any other failure keeps the Node's message, or the plain label.
    expect(
      runFailure({'code': 'internal', 'message': 'broke'}, translate: _key),
      'broke',
    );
    expect(
      runFailure({'code': 'later'}, translate: _key),
      'conversationFailedStatus',
    );
    expect(runFailure(null, translate: _key), 'conversationFailedStatus');
  });

  test('a refused continuation names its reason', () {
    for (final (reason, key) in <(Object?, String)>[
      ('plan', 'continuationPlan'),
      ('child', 'continuationChild'),
      ('archived', 'continuationArchived'),
      ('worktree', 'continuationWorktree'),
      ('queue_paused', 'continuationQueuePaused'),
      ('queued_input', 'continuationQueuedInput'),
      ('running', 'continuationRunning'),
      ('interaction', 'continuationInteraction'),
      ('limit', 'continuationLimit'),
      ('stale', 'continuationStale'),
      ('later', 'continuationRefused'),
      (null, 'continuationRefused'),
    ]) {
      expect(
        continuationRefusal({
          'kind': 'continuation',
          'reason': reason,
        }, translate: _key),
        key,
        reason: '$reason',
      );
    }
    expect(
      continuationRefusal(
        {'kind': 'continuation', 'reason': 'limit', 'count': 8, 'limit': 8},
        translate: (key) =>
            key == 'continuationLimitCount' ? 'Limit {count}/{limit}' : key,
      ),
      'Limit 8/8',
    );
    // Counts belong to the limit; another reason ignores them.
    expect(
      continuationRefusal({
        'kind': 'continuation',
        'reason': 'running',
        'count': 1,
        'limit': 8,
      }, translate: _key),
      'continuationRunning',
    );
    expect(continuationRefusal({'kind': 'model_request'}), isNull);
    expect(continuationRefusal(null), isNull);
    expect(continuationRefusal('continuation'), isNull);
  });

  test('a command failure reads its continuation details first', () {
    expect(
      failureLabel(
        const CommandFailure(
          'conflict',
          details: {'kind': 'continuation', 'reason': 'queue_paused'},
        ),
        translate: _key,
      ),
      'continuationQueuePaused',
    );
    expect(
      failureLabel(
        const CommandFailure('conflict', details: {'kind': 'later'}),
        translate: _key,
      ),
      'conversationFailed',
    );
  });

  test('only a continued turn has a marker', () {
    expect(continuationMarker({'turn': 'a'}), isNull);
    expect(continuationMarker({'continuation': null}), isNull);
    expect(continuationMarker({'continuation': 'goal'}), isNull);
    // A continuation without a declared marker still marks the turn.
    expect(
      continuationMarker({
        'continuation': {'package': {}, 'marker': null},
      }),
      isEmpty,
    );
    expect(
      continuationMarker({
        'continuation': {
          'marker': {
            'label': 'Goal',
            'locales': {'zh-CN': '目标'},
          },
          'kind': 'goal',
          'limit': 8,
          'user': true,
        },
      }),
      {
        'label': 'Goal',
        'locales': {'zh-CN': '目标'},
      },
    );
  });
}
