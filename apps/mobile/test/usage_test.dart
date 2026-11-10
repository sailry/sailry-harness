import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/usage_data.dart';

const _hour = 3600000;

Map<String, dynamic> point(
  DateTime start, {
  int responses = 0,
  Map<String, dynamic>? tokens,
}) => {
  'start_ms': start.millisecondsSinceEpoch,
  'metrics': {'responses': responses, 'tokens': tokens},
  'models': <String, dynamic>{},
};

Map<String, dynamic> usage(int input, int cached, int output) => {
  'input': input,
  'cached_input': cached,
  'output': output,
  'reasoning': 0,
};

Map<String, dynamic> modelGroup(
  String provider,
  String model, {
  String? name,
  int responses = 1,
  Map<String, dynamic>? tokens,
  Map<String, dynamic>? cost,
}) => {
  'key': {
    'kind': 'model',
    'data': {'provider': provider, 'model': model},
  },
  'name': name,
  'metrics': {'responses': responses, 'tokens': tokens, 'cost': cost},
};

void main() {
  group('query', () {
    test("aligns query ranges and device offsets", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "ends a day range at the next local midnight",
          (caseName) {
            final query = usageQuery(
              UsageRange.month,
              DateTime.utc(2026, 10, 20, 9),
            );
            final end = DateTime.utc(2026, 10, 21).millisecondsSinceEpoch;
            expect(query['end_ms'], end, reason: caseName);
            expect(query['start_ms'], end - 30 * 24 * _hour, reason: caseName);
            expect(query['utc_offset_minutes'], 0, reason: caseName);
            expect(query['dimension'], 'model', reason: caseName);
            expect(query['projects'], isEmpty, reason: caseName);
          },
        ),
        (
          "ends 24 hours at the next half hour",
          (caseName) {
            final query = usageQuery(
              UsageRange.day,
              DateTime.utc(2026, 10, 20, 9, 10),
              dimension: 'project',
              project: 'p1',
            );
            final end = DateTime.utc(
              2026,
              10,
              20,
              9,
              30,
            ).millisecondsSinceEpoch;
            expect(query['end_ms'], end, reason: caseName);
            expect(query['start_ms'], end - 24 * _hour, reason: caseName);
            expect(query['dimension'], 'project', reason: caseName);
            expect(query['projects'], ['p1'], reason: caseName);
          },
        ),
        (
          "aligns to the device offset",
          (caseName) {
            final now = DateTime(2026, 10, 20, 9, 10);
            final query = usageQuery(UsageRange.week, now);
            expect(
              query['utc_offset_minutes'],
              now.timeZoneOffset.inMinutes,
              reason: caseName,
            );
            expect(
              DateTime.fromMillisecondsSinceEpoch(query['end_ms'] as int),
              DateTime(2026, 10, 21),
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('format', () {
    test("compacts counts costs and measured speed", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (compact(null), isNull, "compacts and groups counts"),
        (compact(999), '999', "compacts and groups counts"),
        (compact(1000), '1.0k', "compacts and groups counts"),
        (compact(48612904), '48.6M', "compacts and groups counts"),
        (grouped(48612904), '48,612,904', "compacts and groups counts"),
        (grouped(12), '12', "compacts and groups counts"),
        (amount(38429999), '38.42', "truncates amounts to cents"),
        (amount(1234), '0.00', "truncates amounts to cents"),
        (
          tokenSpeed({'elapsed_us': 2000000, 'output_tokens': 84}),
          '42.0',
          "derives speed from timed output",
        ),
        (
          tokenSpeed({'elapsed_us': 0, 'output_tokens': 84}),
          isNull,
          "derives speed from timed output",
        ),
        (tokenSpeed(null), isNull, "derives speed from timed output"),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  group('overview', () {
    test("preserves partial and unknown overview values", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "caps cache at input and marks partial cost",
          (caseName) {
            final overview = UsageOverview({
              'responses': 4,
              'tokens': {
                'input': 100,
                'cached_input': 140,
                'output': 20,
                'reasoning': 5,
              },
              'cost': {'usd_micros': 1500000, 'responses': 3},
              'generation': {'elapsed_us': 1000000, 'output_tokens': 20},
            });
            expect(overview.tokens!.parts, [100, 0, 20], reason: caseName);
            expect(overview.total, 120, reason: caseName);
            expect(overview.reasoning, 5, reason: caseName);
            expect(overview.hit, 100, reason: caseName);
            expect(overview.cost, '1.50', reason: caseName);
            expect(overview.partial, isTrue, reason: caseName);
            expect(overview.speed, '20.0', reason: caseName);
          },
        ),
        (
          "keeps unknown tokens and cost",
          (caseName) {
            final overview = UsageOverview({'responses': 2});
            expect(overview.total, isNull, reason: caseName);
            expect(overview.hit, isNull, reason: caseName);
            expect(overview.cost, isNull, reason: caseName);
            expect(overview.partial, isFalse, reason: caseName);
            expect(overview.shares, [null, null, null], reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('buckets', () {
    test("groups timed buckets and preserves unknown values", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "groups days into Monday weeks at the offset",
          (caseName) {
            // Local days at UTC+8 start at 16:00 UTC on the previous day.
            final days = [
              for (var day = 0; day < 9; day++)
                point(
                  DateTime.utc(2026, 9, 18 + day, 16),
                  responses: day == 0 ? 1 : 0,
                  tokens: day == 0 ? usage(100, 40, 20) : null,
                ),
            ];
            final daily = usageBuckets(days, UsageUnit.day, 480);
            expect(daily.length, 9, reason: caseName);
            expect(
              daily.first.start,
              DateTime.utc(2026, 9, 19),
              reason: caseName,
            );
            expect(daily.first.tokens!.parts, [40, 60, 20], reason: caseName);
            expect(daily.last.tokens!.total, 0, reason: caseName);
            final weekly = usageBuckets(days, UsageUnit.week, 480);
            expect(weekly.length, 2, reason: caseName);
            expect(
              weekly.first.start,
              DateTime.utc(2026, 9, 14),
              reason: caseName,
            );
            expect(
              weekly.first.first,
              DateTime.utc(2026, 9, 19),
              reason: caseName,
            );
            expect(
              weekly.first.last,
              DateTime.utc(2026, 9, 20),
              reason: caseName,
            );
            expect(
              weekly.last.start,
              DateTime.utc(2026, 9, 21),
              reason: caseName,
            );
            expect(weekly.first.tokens!.total, 120, reason: caseName);
          },
        ),
        (
          "marks responses without tokens unknown",
          (caseName) {
            final buckets = usageBuckets(
              [
                point(DateTime.utc(2026, 9, 21), responses: 1),
                point(
                  DateTime.utc(2026, 9, 22),
                  responses: 1,
                  tokens: usage(1, 0, 1),
                ),
              ],
              UsageUnit.week,
              0,
            );
            expect(buckets.single.tokens, isNull, reason: caseName);
          },
        ),
        (
          "labels three-hour marks and recent days",
          (caseName) {
            final hours = usageBuckets(
              [
                for (var slot = 0; slot < 8; slot++)
                  point(
                    DateTime.utc(
                      2026,
                      9,
                      21,
                      2,
                    ).add(Duration(minutes: 30 * slot)),
                  ),
              ],
              UsageUnit.halfHour,
              0,
            );
            expect(usageLabels(hours, UsageUnit.halfHour), [
              '',
              '',
              '03:00',
              '',
              '',
              '',
              '',
              '',
            ], reason: caseName);
            expect(
              usageHeading(hours[2], UsageUnit.halfHour),
              '2026-09-21 03:00–03:30',
              reason: caseName,
            );
            final days = usageBuckets(
              [
                for (var day = 1; day <= 14; day++)
                  point(DateTime.utc(2026, 9, day)),
              ],
              UsageUnit.day,
              0,
            );
            final labels = usageLabels(days, UsageUnit.day);
            expect(labels.last, '9/14', reason: caseName);
            expect(labels[11], '9/12', reason: caseName);
            expect(labels[12], '', reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });

    test('rounds the axis to four divisions', () {
      expect(usageAxis(0), (maximum: 1.0, step: 1.0));
      expect(usageAxis(3900000), (maximum: 4000000.0, step: 1000000.0));
      expect(usageAxis(7), (maximum: 8.0, step: 2.0));
    });
  });

  group('heatmap', () {
    test('places days in week columns', () {
      // 2026-09-23 is a Wednesday.
      final days = [
        for (var day = 0; day < 10; day++)
          point(
            DateTime.utc(2026, 9, 23 + day),
            responses: day.isEven ? 1 : 0,
            tokens: day.isEven ? usage(10 * (day + 1), 0, 0) : null,
          ),
      ];
      final map = usageHeatmap(days, 0);
      expect(map.columns, 2);
      expect(map.active, 5);
      expect(map.cells.first.row, 2);
      expect(map.cells.first.column, 0);
      expect(map.cells.last.column, 1);
      expect(map.cells[8].intensity, 1);
      expect(map.cells[1].intensity, 0);
    });
  });

  group('rows', () {
    final groups = <Map<String, dynamic>>[
      modelGroup(
        'anthropic',
        'opus',
        name: 'Anthropic',
        responses: 2,
        tokens: usage(100, 50, 20),
        cost: {'usd_micros': 2000000, 'responses': 1},
      ),
      {
        'node': [1, 2],
        'group': modelGroup(
          'anthropic',
          'haiku',
          name: 'Anthropic',
          tokens: usage(30, 0, 10),
          cost: {'usd_micros': 500000, 'responses': 1},
        ),
      },
      modelGroup('openai-provider-id', 'sol', tokens: usage(60, 30, 20)),
    ];

    test("aggregates models providers and project owners", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "lists models with provider notes",
          (caseName) {
            final rows = usageRows(
              groups,
              UsageBreakdown.model,
              unassigned: '-',
            );
            expect(rows.map((row) => row.name), [
              'opus',
              'sol',
              'haiku',
            ], reason: caseName);
            expect(rows.first.note, 'Anthropic', reason: caseName);
            expect(rows[1].note, 'openai-p', reason: caseName);
            expect(rows.first.cost, '2.00+', reason: caseName);
            expect(rows.last.cost, '0.50', reason: caseName);
            expect(rows.first.hit, 50, reason: caseName);
            expect(rows.first.share, 50, reason: caseName);
          },
        ),
        (
          "sums providers from model groups",
          (caseName) {
            final rows = usageRows(
              groups,
              UsageBreakdown.provider,
              unassigned: '-',
            );
            expect(rows.length, 2, reason: caseName);
            expect(rows.first.name, 'Anthropic', reason: caseName);
            expect(rows.first.total, 160, reason: caseName);
            expect(rows.first.responses, 3, reason: caseName);
            expect(rows.first.cost, '2.50+', reason: caseName);
            expect(rows.last.cost, isNull, reason: caseName);
          },
        ),
        (
          "names project owners",
          (caseName) {
            final rows = usageRows(
              [
                {
                  'key': {'kind': 'project', 'data': 'project-id'},
                  'metrics': {'responses': 1, 'tokens': usage(10, 0, 0)},
                },
                {
                  'key': {'kind': 'session', 'data': 's1'},
                  'metrics': {'responses': 1, 'tokens': usage(5, 0, 0)},
                },
                {
                  'key': {'kind': 'session', 'data': 's2'},
                  'metrics': {'responses': 1, 'tokens': usage(7, 0, 0)},
                },
                ...groups,
              ],
              UsageBreakdown.project,
              projects: {'project-id': 'Sailry'},
              unassigned: 'Unassigned',
            );
            expect(rows.map((row) => row.name), [
              'Unassigned',
              'Sailry',
            ], reason: caseName);
            expect(rows.first.total, 12, reason: caseName);
            expect(rows.first.responses, 2, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });
}
