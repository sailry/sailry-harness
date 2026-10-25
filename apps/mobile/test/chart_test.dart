import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/charts/data.dart';

Map<String, dynamic> points(String type) => {
  'type': type,
  'title': 'Values',
  'data': [
    {'label': 'A', 'value': 1},
    {'label': 'B', 'value': 2},
    {'label': 'C', 'value': 3},
  ],
};

Map<String, dynamic> candles() => {
  'type': 'candlestick',
  'title': 'Prices',
  'data': [
    {'label': 'Mon', 'open': 2, 'high': 4, 'low': 1, 'close': 3},
    {'label': 'Tue', 'open': 3, 'high': 5, 'low': 2, 'close': 2},
  ],
};

Map<String, dynamic> graph() => {
  'type': 'sankey',
  'title': 'Flow',
  'nodes': [
    {'id': 'a', 'label': 'A'},
    {'id': 'b', 'label': 'B'},
  ],
  'links': [
    {'source': 'a', 'target': 'b', 'value': 2},
  ],
};

void main() {
  group('schema', () {
    test('accepts every supported shape', () {
      for (final type in ['line', 'bar', 'area', 'pie', 'radar']) {
        final spec = ChartSpec.parse(jsonEncode(points(type)))!;
        expect(spec.type.name, type);
        expect(spec.title, 'Values');
        expect(spec.points.map((point) => point.value), [1, 2, 3]);
      }
      final candle = ChartSpec.parse(jsonEncode(candles()))!;
      expect(candle.candles.first.open, 2);
      expect(candle.candles.first.high, 4);
      expect(candle.candles.first.low, 1);
      expect(candle.candles.first.close, 3);
      final flow = ChartSpec.parse(jsonEncode(graph()))!;
      expect(flow.nodes.length, 2);
      expect(flow.links.single.value, 2);
    });
    test('rejects unknown and incomplete fields', () {
      final values = points('line');
      for (final invalid in [
        'null',
        '[]',
        '{',
        jsonEncode({...values, 'unknown': true}),
        jsonEncode({...values, 'title': null}),
        jsonEncode({...values, 'type': 'scatter'}),
        jsonEncode({'type': 'line'}),
        jsonEncode({'type': 'line', 'data': {}}),
        jsonEncode({'type': 'line', 'data': []}),
        jsonEncode({
          'type': 'line',
          'data': [
            {'label': 'A', 'value': 1, 'unknown': true},
          ],
        }),
        jsonEncode({
          'type': 'line',
          'data': [
            {'value': 1},
          ],
        }),
        jsonEncode({
          'type': 'line',
          'data': [
            {'label': 'A', 'value': '1'},
          ],
        }),
        jsonEncode({...candles(), 'nodes': []}),
        jsonEncode({...graph(), 'data': []}),
      ]) {
        expect(ChartSpec.parse(invalid), isNull, reason: invalid);
      }
    });
    test('enforces UTF8 source and label limits', () {
      final base = jsonEncode({
        'type': 'line',
        'data': [
          {'label': '', 'value': 0},
        ],
      });
      expect(ChartSpec.parse(base.padRight(64 * 1024)), isNotNull);
      expect(ChartSpec.parse(base.padRight(64 * 1024 + 1)), isNull);
      for (final (label, accepted) in [
        ('a' * 256, true),
        ('a' * 257, false),
        ('汉' * 85, true),
        ('汉' * 86, false),
        ('😀' * 64, true),
        ('😀' * 65, false),
        ('A\nB', false),
        ('A\u007fB', false),
        ('A\u009fB', false),
        ('A\u200dB', true),
      ]) {
        expect(
          ChartSpec.parse(
                jsonEncode({
                  'type': 'line',
                  'title': label,
                  'data': [
                    {'label': label, 'value': 0},
                  ],
                }),
              ) !=
              null,
          accepted,
        );
      }
      expect(
        ChartSpec.parse(
          r'{"type":"line","data":[{"label":"\ud800","value":0}]}',
        ),
        isNull,
      );
      final unicode = jsonEncode({
        'type': 'line',
        'data': List.generate(400, (_) => {'label': '汉' * 80, 'value': 0}),
      });
      expect(unicode.length, lessThan(64 * 1024));
      expect(utf8.encode(unicode).length, greaterThan(64 * 1024));
      expect(ChartSpec.parse(unicode), isNull);
    });
    test('bounds values and data counts', () {
      for (final type in ['line', 'bar', 'area']) {
        final data = {
          'type': type,
          'data': [
            {'label': 'A', 'value': -1e12},
            {'label': 'B', 'value': 1e12},
          ],
        };
        expect(ChartSpec.parse(jsonEncode(data)), isNotNull);
      }
      for (final value in ['1e309', '-1e309', '1000000000001']) {
        expect(
          ChartSpec.parse(
            '{"type":"line","data":[{"label":"A","value":$value}]}',
          ),
          isNull,
        );
      }
      for (final count in [500, 501]) {
        expect(
          ChartSpec.parse(
                jsonEncode({
                  'type': 'line',
                  'data': List.generate(
                    count,
                    (_) => {'label': '', 'value': 0},
                  ),
                }),
              ) !=
              null,
          count == 500,
        );
        expect(
          ChartSpec.parse(
                jsonEncode({
                  'type': 'candlestick',
                  'data': List.generate(
                    count,
                    (_) => {
                      'label': '',
                      'open': 0,
                      'high': 0,
                      'low': 0,
                      'close': 0,
                    },
                  ),
                }),
              ) !=
              null,
          count == 500,
        );
      }
      for (final type in ['pie', 'radar']) {
        for (final values in [
          <num>[0, 0, 0],
          <num>[-1, 2, 3],
        ]) {
          expect(
            ChartSpec.parse(
              jsonEncode({
                'type': type,
                'data': [
                  for (final value in values) {'label': '', 'value': value},
                ],
              }),
            ),
            isNull,
          );
        }
      }
      expect(
        ChartSpec.parse(
          jsonEncode({
            'type': 'radar',
            'data': [
              {'label': 'A', 'value': 1},
              {'label': 'B', 'value': 2},
            ],
          }),
        ),
        isNull,
      );
      for (final invalid in [
        {'label': 'A', 'open': 2, 'high': 1, 'low': 0, 'close': 3},
        {'label': 'A', 'open': 2, 'high': 4, 'low': 3, 'close': 3},
        {'label': 'A', 'open': 2, 'high': 4, 'low': 1},
        {'label': 'A', 'open': 2, 'high': 4, 'low': 1, 'close': 3, 'extra': 0},
      ]) {
        expect(
          ChartSpec.parse(
            jsonEncode({
              'type': 'candlestick',
              'data': [invalid],
            }),
          ),
          isNull,
        );
      }
    });
    test('accepts only bounded acyclic graphs', () {
      final base = graph();
      for (final invalid in [
        {...base, 'nodes': []},
        {...base, 'links': []},
        {
          ...base,
          'nodes': [
            {'id': 'a', 'label': 'A'},
            {'id': 'a', 'label': 'B'},
          ],
        },
        {
          ...base,
          'nodes': [
            {'id': '', 'label': 'A'},
            {'id': 'b', 'label': 'B'},
          ],
        },
        {
          ...base,
          'nodes': [
            {'id': 'a', 'label': 'A', 'extra': 0},
            {'id': 'b', 'label': 'B'},
          ],
        },
        for (final link in [
          {'source': 'a', 'target': 'missing', 'value': 1},
          {'source': 'a', 'target': 'a', 'value': 1},
          {'source': 'a', 'target': 'b', 'value': 0},
          {'source': 'a', 'target': 'b', 'value': -1},
          {'source': 'a', 'target': 'b', 'value': 1, 'extra': 0},
        ])
          {
            ...base,
            'links': [link],
          },
        {
          ...base,
          'links': [
            {'source': 'a', 'target': 'b', 'value': 1},
            {'source': 'b', 'target': 'a', 'value': 2},
          ],
        },
      ]) {
        expect(ChartSpec.parse(jsonEncode(invalid)), isNull);
      }
      for (final count in [64, 65]) {
        expect(
          ChartSpec.parse(
                jsonEncode({
                  ...base,
                  'nodes': [
                    {'id': 'a', 'label': 'A'},
                    {'id': 'b', 'label': 'B'},
                    for (var index = 2; index < count; index++)
                      {'id': '$index', 'label': ''},
                  ],
                }),
              ) !=
              null,
          count == 64,
        );
      }
      for (final count in [128, 129]) {
        expect(
          ChartSpec.parse(
                jsonEncode({
                  ...base,
                  'links': List.generate(
                    count,
                    (_) => {'source': 'a', 'target': 'b', 'value': 1},
                  ),
                }),
              ) !=
              null,
          count == 128,
        );
      }
    });
  });
}
