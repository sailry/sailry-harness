import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/usage_data.dart';

void main() {
  test('daily and weekly usage preserves cache and zero days', () {
    final days = [
      for (var i = 0; i < 9; i++)
        {
          'start_ms': DateTime.utc(2026, 9, 19 + i).millisecondsSinceEpoch,
          'metrics': {
            'responses': i == 0 ? 1 : 0,
            'tokens': i == 0
                ? {
                    'input': 100,
                    'cached_input': 40,
                    'output': 20,
                    'reasoning': 10,
                  }
                : null,
          },
        },
    ];
    final daily = usageBuckets(days);
    expect(daily.length, 9);
    expect(daily.first.tokens!.cache, 40);
    expect(daily.first.tokens!.input, 60);
    expect(daily.first.tokens!.output, 20);
    expect(daily.first.tokens!.total, 120);
    expect(daily.last.tokens!.total, 0);
    final weekly = usageBuckets(days, weekly: true);
    expect(weekly.length, 2);
    expect(weekly.first.end, DateTime.utc(2026, 9, 20));
    expect(weekly.last.start, DateTime.utc(2026, 9, 21));
    expect(weekly.first.tokens!.total, 120);
  });
}
