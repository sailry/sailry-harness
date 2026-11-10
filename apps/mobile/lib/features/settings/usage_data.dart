// Presentation projections of the shared Client's canonical usage reports.
// Range, bucket, summary and breakdown policy follows the desktop statistics
// plugin (plugins/statistics/dev.sailry.platform/desktop/{state,format,charts,
// summary,breakdown}.js) so both controllers show the same figures.
import 'dart:convert';
import 'dart:math' as math;
import 'live.dart';

const _minute = 60000;
const _halfHour = 1800000;
const _day = 86400000;

enum UsageUnit { halfHour, day, week }

/// Range length and the trend bucket that keeps its bars readable.
enum UsageRange {
  day(1, UsageUnit.halfHour, 'usageRange24h'),
  week(7, UsageUnit.day, 'usageRange7d'),
  month(30, UsageUnit.day, 'usageRange30d'),
  quarter(90, UsageUnit.week, 'usageRange90d'),
  year(365, UsageUnit.week, 'usageRange1y');

  const UsageRange(this.days, this.unit, this.label);
  final int days;
  final UsageUnit unit;
  final String label;
}

enum UsageBreakdown { model, provider, project }

/// Provider rows sum model groups locally; only project rows need another grouping.
String usageDimension(UsageBreakdown breakdown) =>
    breakdown == UsageBreakdown.project ? 'project' : 'model';

/// Query whose window ends at the next local boundary, so the current bucket
/// is complete; the Node buckets days and half hours at the viewer's offset.
Map<String, dynamic> usageQuery(
  UsageRange range,
  DateTime now, {
  String dimension = 'model',
  String? project,
}) {
  final minutes = now.timeZoneOffset.inMinutes;
  final shift = minutes * _minute;
  final width = range == UsageRange.day ? _halfHour : _day;
  final end =
      ((now.millisecondsSinceEpoch + shift) ~/ width + 1) * width - shift;
  return {
    'start_ms': end - range.days * _day,
    'end_ms': end,
    'utc_offset_minutes': minutes,
    'dimension': dimension,
    'projects': [?project],
    'worktrees': [],
    'providers': [],
    'models': [],
    'before': null,
  };
}

/// Compact count such as 48.6M; unknown values stay null.
String? compact(int? value) {
  if (value == null) return null;
  for (final (scale, suffix) in const [
    (1000000000000, 'T'),
    (1000000000, 'B'),
    (1000000, 'M'),
    (1000, 'k'),
  ]) {
    if (value >= scale) return '${(value / scale).toStringAsFixed(1)}$suffix';
  }
  return '$value';
}

/// Exact integer with thousands separators.
String? grouped(int? value) => value?.toString().replaceAllMapped(
  RegExp(r'\B(?=(\d{3})+(?!\d))'),
  (_) => ',',
);

/// Percentage of [part] in [whole], or null without a positive whole.
double? ratio(int? part, int? whole) =>
    part == null || whole == null || whole <= 0 ? null : part * 100 / whole;

/// One-decimal percentage label.
String? percent(double? value) =>
    value == null ? null : '${value.toStringAsFixed(1)}%';

/// USD amount truncated to cents.
String? amount(int? micros) => micros == null
    ? null
    : '${micros ~/ 1000000}.${(micros % 1000000 ~/ 10000).toString().padLeft(2, '0')}';

/// Average output tokens per second over timed responses.
String? tokenSpeed(Map<String, dynamic>? generation) {
  final elapsed = integer(generation?['elapsed_us']);
  if (elapsed <= 0) return null;
  return (integer(generation?['output_tokens']) * 1000000 / elapsed)
      .toStringAsFixed(1);
}

/// Disjoint token parts: cached input capped at input, uncached input and output.
class TokenStack {
  const TokenStack(this.cache, this.input, this.output);
  static TokenStack? from(Object? tokens) {
    if (tokens is! Map) return null;
    final input = integer(tokens['input']);
    final cache = math.min(input, integer(tokens['cached_input']));
    return TokenStack(cache, input - cache, integer(tokens['output']));
  }

  final int cache;
  final int input;
  final int output;
  int get total => cache + input + output;
  List<int> get parts => [cache, input, output];
  TokenStack operator +(TokenStack other) => TokenStack(
    cache + other.cache,
    input + other.input,
    output + other.output,
  );
}

/// Token composition, cache hit rate, speed and estimated cost for one report.
class UsageOverview {
  factory UsageOverview(Map<String, dynamic> totals) {
    final tokens = TokenStack.from(totals['tokens']);
    final responses = integer(totals['responses']);
    final cost = totals['cost'] is Map ? object(totals['cost']) : null;
    final priced = integer(cost?['responses']);
    return UsageOverview._(
      tokens: tokens,
      reasoning: totals['tokens'] is Map
          ? integer(object(totals['tokens'])['reasoning'])
          : null,
      hit: tokens == null
          ? null
          : ratio(tokens.cache, tokens.cache + tokens.input),
      responses: responses,
      speed: tokenSpeed(
        totals['generation'] is Map ? object(totals['generation']) : null,
      ),
      cost: cost == null ? null : amount(integer(cost['usd_micros'])),
      priced: priced,
      partial: cost != null && priced < responses,
    );
  }
  const UsageOverview._({
    required this.tokens,
    required this.reasoning,
    required this.hit,
    required this.responses,
    required this.speed,
    required this.cost,
    required this.priced,
    required this.partial,
  });

  final TokenStack? tokens;
  final int? reasoning;
  final double? hit;
  final int responses;
  final String? speed;
  final String? cost;
  final int priced;

  /// Estimated cost covers only some responses.
  final bool partial;

  int? get total => tokens?.total;
  List<double?> get shares => [
    for (final part in tokens?.parts ?? const [null, null, null])
      ratio(part, total),
  ];
}

/// Local wall-clock time of a UTC instant at a fixed offset, carried in UTC fields.
DateTime _wall(int milliseconds, int offset) =>
    DateTime.fromMillisecondsSinceEpoch(
      milliseconds + offset * _minute,
      isUtc: true,
    );

/// Trend bucket; dates are local wall-clock times in UTC fields.
class UsageBucket {
  UsageBucket(this.start, this.first, this.last, this.tokens);
  final DateTime start;
  DateTime first;
  DateTime last;

  /// Null when a response in the bucket reported no tokens.
  TokenStack? tokens;
}

/// Groups report points into half-hour, day or Monday-based week buckets at one fixed offset.
List<UsageBucket> usageBuckets(
  List<Map<String, dynamic>> points,
  UsageUnit unit,
  int offset,
) {
  final buckets = <int, UsageBucket>{};
  for (final point in points) {
    final milliseconds = integer(point['start_ms']);
    final date = _wall(milliseconds, offset);
    // Calendar arithmetic runs on the wall clock; subtracting the offset
    // recovers the UTC Monday boundary.
    final start = unit == UsageUnit.week
        ? DateTime.utc(
                date.year,
                date.month,
                date.day - (date.weekday - DateTime.monday),
              ).millisecondsSinceEpoch -
              offset * _minute
        : milliseconds;
    final metrics = object(point['metrics']);
    final tokens =
        TokenStack.from(metrics['tokens']) ??
        (integer(metrics['responses']) == 0 ? const TokenStack(0, 0, 0) : null);
    final bucket = buckets[start];
    if (bucket == null) {
      buckets[start] = UsageBucket(_wall(start, offset), date, date, tokens);
      continue;
    }
    if (date.isBefore(bucket.first)) bucket.first = date;
    if (date.isAfter(bucket.last)) bucket.last = date;
    final previous = bucket.tokens;
    bucket.tokens = previous == null || tokens == null
        ? null
        : previous + tokens;
  }
  return buckets.values.toList()..sort((a, b) => a.start.compareTo(b.start));
}

String _pad(int value) => value.toString().padLeft(2, '0');
String _date(DateTime date) =>
    '${date.year}-${_pad(date.month)}-${_pad(date.day)}';
String _clock(DateTime date) => '${_pad(date.hour)}:${_pad(date.minute)}';

/// Sparse axis labels: three-hour marks, or about seven labels counted back
/// from the newest bucket. Empty strings mark unlabeled buckets.
List<String> usageLabels(List<UsageBucket> buckets, UsageUnit unit) {
  if (unit == UsageUnit.halfHour) {
    return [
      for (final bucket in buckets)
        bucket.start.minute == 0 && bucket.start.hour % 3 == 0
            ? _clock(bucket.start)
            : '',
    ];
  }
  final step = math.max(1, (buckets.length / 7).ceil());
  return [
    for (var index = 0; index < buckets.length; index++)
      (buckets.length - 1 - index) % step == 0
          ? '${buckets[index].start.month}/${buckets[index].start.day}'
          : '',
  ];
}

/// Tooltip heading for one bucket.
String usageHeading(UsageBucket bucket, UsageUnit unit) {
  if (unit == UsageUnit.halfHour) {
    final end = bucket.start.add(const Duration(minutes: 30));
    return '${_date(bucket.start)} ${_clock(bucket.start)}–${_clock(end)}';
  }
  final first = _date(bucket.first);
  final last = _date(bucket.last);
  return first == last ? first : '$first – $last';
}

/// Rounded axis maximum with at most four integer divisions.
({double maximum, double step}) usageAxis(int peak) {
  if (peak <= 0) return (maximum: 1, step: 1);
  final raw = peak / 4;
  final power = math.pow(10, (math.log(raw) / math.ln10).floor()).toDouble();
  final step = math.max(
    1.0,
    <double>[1, 2, 2.5, 5, 10]
        .map((factor) => factor * power)
        .firstWhere((value) => value >= raw, orElse: () => 10 * power),
  );
  return (maximum: step * math.max(1, (peak / step).ceil()), step: step);
}

/// One day cell of the yearly heatmap.
class UsageCell {
  const UsageCell(this.column, this.row, this.intensity);
  final int column;
  final int row;

  /// 0 for no tokens, otherwise one of four increasing levels up to 1.
  final double intensity;
}

/// Day cells with one column per Monday-based week.
({int columns, List<UsageCell> cells, int active}) usageHeatmap(
  List<Map<String, dynamic>> days,
  int offset,
) {
  if (days.isEmpty) return (columns: 1, cells: const [], active: 0);
  int calendar(DateTime date) =>
      DateTime.utc(date.year, date.month, date.day).millisecondsSinceEpoch ~/
      _day;
  final first = _wall(integer(days.first['start_ms']), offset);
  final start = calendar(first) - (first.weekday - DateTime.monday);
  final entries = <(int, int?)>[];
  for (final day in days) {
    final date = _wall(integer(day['start_ms']), offset);
    final position = calendar(date) - start;
    if (position < 0) continue;
    final metrics = object(day['metrics']);
    final tokens = metrics['tokens'] is Map
        ? integer(object(metrics['tokens'])['input']) +
              integer(object(metrics['tokens'])['output'])
        : integer(metrics['responses']) == 0
        ? 0
        : null;
    entries.add((position, tokens));
  }
  final maximum = entries.fold<int>(
    0,
    (maximum, entry) => math.max(maximum, entry.$2 ?? 0),
  );
  const levels = [0.0, .25, .45, .7, 1.0];
  return (
    columns: entries.isEmpty ? 1 : entries.last.$1 ~/ 7 + 1,
    cells: [
      for (final (position, tokens) in entries)
        UsageCell(
          position ~/ 7,
          position % 7,
          levels[(tokens ?? 0) > 0
              ? math.min(4, (math.sqrt(tokens! / maximum) * 4).ceil())
              : 0],
        ),
    ],
    active: entries.where((entry) => (entry.$2 ?? 0) > 0).length,
  );
}

/// One breakdown row with its share of the listed total.
class UsageRow {
  UsageRow(this.id, this.name, this.note);
  final String id;
  final String name;

  /// Provider name for model rows.
  final String? note;
  int responses = 0;
  Map<String, int>? _tokens;
  int? _usd;
  int _priced = 0;
  double? share;

  TokenStack? get tokens => TokenStack.from(_tokens);
  int? get total => tokens?.total;
  double? get hit {
    final value = tokens;
    return value == null ? null : ratio(value.cache, value.cache + value.input);
  }

  /// Estimated cost; '+' marks rows where only some responses are priced.
  String? get cost =>
      _usd == null ? null : '${amount(_usd)}${_priced < responses ? '+' : ''}';

  void _add(Map<String, dynamic> metrics) {
    responses += integer(metrics['responses']);
    if (metrics['tokens'] case final Map<String, dynamic> tokens) {
      final sum = _tokens ??= {
        'input': 0,
        'output': 0,
        'cached_input': 0,
        'reasoning': 0,
      };
      for (final key in sum.keys.toList()) {
        sum[key] = sum[key]! + integer(tokens[key]);
      }
    }
    if (metrics['cost'] case final Map<String, dynamic> cost) {
      _usd = (_usd ?? 0) + integer(cost['usd_micros']);
      _priced += integer(cost['responses']);
    }
  }
}

String _short(String value) => value.length > 8 ? value.substring(0, 8) : value;

/// Breakdown rows sorted by token totals. Report groups may be wrapped with
/// their Node when combined across hosts.
List<UsageRow> usageRows(
  List<Map<String, dynamic>> groups,
  UsageBreakdown mode, {
  Map<String, String> projects = const {},
  required String unassigned,
}) {
  final rows = <String, UsageRow>{};
  for (final entry in groups) {
    final group = entry.containsKey('group') ? object(entry['group']) : entry;
    final key = object(group['key']);
    final kind = string(key['kind']);
    final data = key['data'];
    final name = group['name'] is String ? group['name'] as String : null;
    UsageRow? row;
    if (kind == 'model' && data is Map) {
      final provider = string(data['provider']);
      final label = name ?? _short(provider);
      row = switch (mode) {
        UsageBreakdown.model => UsageRow(
          '$provider:${string(data['model'])}',
          string(data['model']),
          label,
        ),
        UsageBreakdown.provider => UsageRow(provider, label, null),
        UsageBreakdown.project => null,
      };
    } else if (mode == UsageBreakdown.project) {
      // Conversations without a project, connection or assistant share one row.
      row = switch (kind) {
        'session' => UsageRow('session', unassigned, null),
        'project' => UsageRow(
          'project:$data',
          name ?? projects[data] ?? _short(string(data)),
          null,
        ),
        'connection' || 'assistant' => UsageRow(
          '$kind:${jsonEncode(data)}',
          name ?? unassigned,
          null,
        ),
        _ => null,
      };
    }
    if (row == null) continue;
    (rows[row.id] ??= row)._add(object(group['metrics']));
  }
  final list = rows.values.toList();
  final sum = list.fold<int>(0, (sum, row) => sum + (row.total ?? 0));
  for (final row in list) {
    row.share = ratio(row.total, sum);
  }
  return list..sort((left, right) {
    final a = left.total;
    final b = right.total;
    if (a != b) {
      if (a == null) return 1;
      if (b == null) return -1;
      return b.compareTo(a);
    }
    final names = left.name.compareTo(right.name);
    return names != 0 ? names : left.id.compareTo(right.id);
  });
}
