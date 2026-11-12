import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/background_data.dart';

void main() {
  test('background state classifies notices and outcomes', () {
    expect(noticeTurn({}), isFalse);
    expect(noticeTurn({'notice': true}), isTrue);
    expect(backgroundLabel('unknown'), 'backgroundUnknown');
    expect(backgroundLabel('timed_out'), 'backgroundTimedOut');
    expect(backgroundLabel('stalled'), 'backgroundStalled');
    for (final outcome in ['failed', 'timed_out', 'interrupted', 'unknown']) {
      expect(backgroundFailed(outcome), isTrue, reason: outcome);
      expect(backgroundWarning(outcome), isFalse, reason: outcome);
    }
    for (final outcome in ['completed', 'cancelled', 'stopped']) {
      expect(backgroundFailed(outcome), isFalse, reason: outcome);
      expect(backgroundWarning(outcome), isFalse, reason: outcome);
    }
    expect(backgroundFailed('stalled'), isFalse);
    expect(backgroundWarning('stalled'), isTrue);
  });

  test('a held result reads as held with its reason', () {
    String tr(String key) => key;
    final held = {'state': 'pending', 'suppressed': true, 'hold': 'limit'};
    expect(backgroundHeld(held), isTrue);
    expect(backgroundHeld({'state': 'pending', 'suppressed': true}), isTrue);
    expect(backgroundHeld({'state': 'pending', 'hold': 'later'}), isTrue);
    // A result on its way to a notice turn, or one already taken, is not held.
    expect(backgroundHeld({'state': 'pending'}), isFalse);
    expect(backgroundHeld({'state': 'delivered', 'hold': 'limit'}), isFalse);
    expect(backgroundHeld({'state': 'running'}), isFalse);
    expect(backgroundHeld({}), isFalse);

    for (final (hold, key) in <(Object?, String)>[
      ('limit', 'backgroundHoldLimit'),
      ('stopped', 'backgroundHoldStopped'),
      ('interrupted', 'backgroundHoldInterrupted'),
      ('failed', 'backgroundHoldFailed'),
      ('other', 'backgroundHeld'),
      ('later', 'backgroundHeld'),
      (null, 'backgroundHeld'),
      (7, 'backgroundHeld'),
    ]) {
      expect(backgroundHoldLabel(hold), key, reason: '$hold');
    }

    expect(
      backgroundHoldText({...held, 'outcome': 'completed'}, tr),
      'backgroundCompleted · backgroundHoldLimit',
    );
    // Without an outcome the line names the hold, never an unknown outcome.
    expect(backgroundHoldText(held, tr), 'backgroundHoldLimit');
    expect(
      backgroundHoldText({'state': 'pending', 'suppressed': true}, tr),
      'backgroundHeld',
    );
  });
}
