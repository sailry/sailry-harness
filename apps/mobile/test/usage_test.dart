import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/usage_data.dart';

const _hour = 3600000;

Map<String, dynamic> point(DateTime start, {int responses = 0, Map? tokens}) =>
    {
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
  Map? tokens,
  Map? cost,
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
    test('ends a day range at the next local midnight', () {
      final query = usageQuery(UsageRange.month, DateTime.utc(2026, 10, 20, 9));
      final end = DateTime.utc(2026, 10, 21).millisecondsSinceEpoch;
      expect(query['end_ms'], end);
      expect(query['start_ms'], end - 30 * 24 * _hour);
      expect(query['utc_offset_minutes'], 0);
      expect(query['dimension'], 'model');
      expect(query['projects'], isEmpty);
    });

    test('ends 24 hours at the next half hour', () {
      final query = usageQuery(
        UsageRange.day,
        DateTime.utc(2026, 10, 20, 9, 10),
        dimension: 'project',
        project: 'p1',
      );
      final end = DateTime.utc(2026, 10, 20, 9, 30).millisecondsSinceEpoch;
      expect(query['end_ms'], end);
      expect(query['start_ms'], end - 24 * _hour);
      expect(query['dimension'], 'project');
      expect(query['projects'], ['p1']);
    });

    test('aligns to the device offset', () {
      final now = DateTime(2026, 10, 20, 9, 10);
      final query = usageQuery(UsageRange.week, now);
      expect(query['utc_offset_minutes'], now.timeZoneOffset.inMinutes);
      expect(
        DateTime.fromMillisecondsSinceEpoch(query['end_ms'] as int),
        DateTime(2026, 10, 21),
      );
    });
  });

  group('format', () {
    test('compacts and groups counts', () {
      expect(compact(null), isNull);
      expect(compact(999), '999');
      expect(compact(1000), '1.0k');
      expect(compact(48612904), '48.6M');
      expect(grouped(48612904), '48,612,904');
      expect(grouped(12), '12');
    });

    test('truncates amounts to cents', () {
      expect(amount(38429999), '38.42');
      expect(amount(1234), '0.00');
    });

    test('derives speed from timed output', () {
      expect(tokenSpeed({'elapsed_us': 2000000, 'output_tokens': 84}), '42.0');
      expect(tokenSpeed({'elapsed_us': 0, 'output_tokens': 84}), isNull);
      expect(tokenSpeed(null), isNull);
    });
  });

  group('overview', () {
    test('caps cache at input and marks partial cost', () {
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
      expect(overview.tokens!.parts, [100, 0, 20]);
      expect(overview.total, 120);
      expect(overview.reasoning, 5);
      expect(overview.hit, 100);
      expect(overview.cost, '1.50');
      expect(overview.partial, isTrue);
      expect(overview.speed, '20.0');
    });

    test('keeps unknown tokens and cost', () {
      final overview = UsageOverview({'responses': 2});
      expect(overview.total, isNull);
      expect(overview.hit, isNull);
      expect(overview.cost, isNull);
      expect(overview.partial, isFalse);
      expect(overview.shares, [null, null, null]);
    });
  });

  group('buckets', () {
    test('groups days into Monday weeks at the offset', () {
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
      expect(daily.length, 9);
      expect(daily.first.start, DateTime.utc(2026, 9, 19));
      expect(daily.first.tokens!.parts, [40, 60, 20]);
      expect(daily.last.tokens!.total, 0);
      final weekly = usageBuckets(days, UsageUnit.week, 480);
      expect(weekly.length, 2);
      expect(weekly.first.start, DateTime.utc(2026, 9, 14));
      expect(weekly.first.first, DateTime.utc(2026, 9, 19));
      expect(weekly.first.last, DateTime.utc(2026, 9, 20));
      expect(weekly.last.start, DateTime.utc(2026, 9, 21));
      expect(weekly.first.tokens!.total, 120);
    });

    test('marks responses without tokens unknown', () {
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
      expect(buckets.single.tokens, isNull);
    });

    test('labels three-hour marks and recent days', () {
      final hours = usageBuckets(
        [
          for (var slot = 0; slot < 8; slot++)
            point(
              DateTime.utc(2026, 9, 21, 2).add(Duration(minutes: 30 * slot)),
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
      ]);
      expect(
        usageHeading(hours[2], UsageUnit.halfHour),
        '2026-09-21 03:00–03:30',
      );
      final days = usageBuckets(
        [for (var day = 1; day <= 14; day++) point(DateTime.utc(2026, 9, day))],
        UsageUnit.day,
        0,
      );
      final labels = usageLabels(days, UsageUnit.day);
      expect(labels.last, '9/14');
      expect(labels[11], '9/12');
      expect(labels[12], '');
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

    test('lists models with provider notes', () {
      final rows = usageRows(groups, UsageBreakdown.model, unassigned: '-');
      expect(rows.map((row) => row.name), ['opus', 'sol', 'haiku']);
      expect(rows.first.note, 'Anthropic');
      expect(rows[1].note, 'openai-p');
      expect(rows.first.cost, '2.00+');
      expect(rows.last.cost, '0.50');
      expect(rows.first.hit, 50);
      expect(rows.first.share, 50);
    });

    test('sums providers from model groups', () {
      final rows = usageRows(groups, UsageBreakdown.provider, unassigned: '-');
      expect(rows.length, 2);
      expect(rows.first.name, 'Anthropic');
      expect(rows.first.total, 160);
      expect(rows.first.responses, 3);
      expect(rows.first.cost, '2.50+');
      expect(rows.last.cost, isNull);
    });

    test('names project owners', () {
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
      expect(rows.map((row) => row.name), ['Unassigned', 'Sailry']);
      expect(rows.first.total, 12);
      expect(rows.first.responses, 2);
    });
  });
}
