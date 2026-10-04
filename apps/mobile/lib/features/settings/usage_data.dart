import 'dart:math' as math;
import 'live.dart';

/// Chart-only projection of the shared Client's canonical usage totals.
class TokenStack {
  const TokenStack(this.cache, this.input, this.output);
  factory TokenStack.from(Map<String, dynamic> tokens) {
    final input = integer(tokens['input']);
    final cache = math.min(input, integer(tokens['cached_input']));
    return TokenStack(cache, input - cache, integer(tokens['output']));
  }
  final int cache;
  final int input;
  final int output;
  int get total => cache + input + output;
  TokenStack operator +(TokenStack other) => TokenStack(
    cache + other.cache,
    input + other.input,
    output + other.output,
  );
}

class UsageBucket {
  UsageBucket(this.start, this.end, this.tokens, this.responses);
  DateTime start;
  DateTime end;
  TokenStack? tokens;
  int responses;
}

List<UsageBucket> usageBuckets(
  List<Map<String, dynamic>> days, {
  bool weekly = false,
}) {
  final buckets = <UsageBucket>[];
  DateTime? previous;
  for (final day in days) {
    final start = DateTime.fromMillisecondsSinceEpoch(
      integer(day['start_ms']),
      isUtc: true,
    );
    final week = start.subtract(
      Duration(days: start.weekday - DateTime.monday),
    );
    final metrics = object(day['metrics']);
    final responses = integer(metrics['responses']);
    final tokens = metrics['tokens'] == null
        ? (responses == 0 ? const TokenStack(0, 0, 0) : null)
        : TokenStack.from(object(metrics['tokens']));
    if (weekly && previous == week) {
      final last = buckets.last;
      last.end = start;
      last.responses += responses;
      last.tokens = last.tokens == null || tokens == null
          ? null
          : last.tokens! + tokens;
    } else {
      buckets.add(UsageBucket(start, start, tokens, responses));
    }
    previous = week;
  }
  return buckets;
}

String usageDate(DateTime date) =>
    '${date.month.toString().padLeft(2, '0')}-${date.day.toString().padLeft(2, '0')}';
