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
}
