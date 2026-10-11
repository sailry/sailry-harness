import 'dart:convert';

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/chart.dart';
import 'package:sailry_mobile/content/charts/data.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/ui/theme.dart';

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

Future<void> mount(
  WidgetTester tester,
  String source, {
  Brightness brightness = Brightness.light,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      home: Scaffold(
        body: SingleChildScrollView(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: SailryChart(source: source),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

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

  for (final brightness in Brightness.values) {
    testWidgets('native charts use the current $brightness theme', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(320, 700);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final theme = SailryTheme.of(brightness);
      for (final type in [
        'line',
        'area',
        'bar',
        'pie',
        'radar',
        'candlestick',
        'sankey',
      ]) {
        final source = jsonEncode(switch (type) {
          'candlestick' => candles(),
          'sankey' => graph(),
          _ => points(type),
        });
        await mount(tester, source, brightness: brightness);
        expect(find.byKey(ValueKey('chart-$type')), findsOneWidget);
        expect(find.byType(CodeBlock), findsNothing);
        if (type == 'line' || type == 'area') {
          final data = tester.widget<LineChart>(find.byType(LineChart)).data;
          expect(data.lineBarsData.single.color, theme.colorScheme.secondary);
          expect(data.lineBarsData.single.spots.map((point) => point.y), [
            1,
            2,
            3,
          ]);
          expect(data.lineBarsData.single.belowBarData.show, type == 'area');
        } else if (type == 'bar') {
          final data = tester.widget<BarChart>(find.byType(BarChart)).data;
          expect(
            data.barGroups.first.barRods.single.color,
            theme.colorScheme.secondary,
          );
          expect(data.barGroups.map((group) => group.barRods.single.toY), [
            1,
            2,
            3,
          ]);
        } else if (type == 'pie') {
          final data = tester.widget<PieChart>(find.byType(PieChart)).data;
          expect(data.sections.first.color, theme.colorScheme.secondary);
          expect(data.sections.map((section) => section.value), [1, 2, 3]);
        } else if (type == 'radar') {
          final data = tester.widget<RadarChart>(find.byType(RadarChart)).data;
          expect(data.dataSets.first.borderColor, theme.colorScheme.secondary);
          expect(data.dataSets.first.dataEntries.map((entry) => entry.value), [
            1,
            2,
            3,
          ]);
          expect(data.isMinValueAtCenter, isTrue);
          expect(
            data.dataSets.last.dataEntries.every((entry) => entry.value == 0),
            isTrue,
          );
        } else if (type == 'candlestick') {
          final data = tester
              .widget<CandlestickChart>(find.byType(CandlestickChart))
              .data;
          final style = (data.candlestickPainter as DefaultCandlestickPainter)
              .candlestickStyleProvider;
          expect(
            style(data.candlestickSpots.first, 0).lineColor,
            theme.colorScheme.tertiary,
          );
          expect(
            style(data.candlestickSpots.last, 1).lineColor,
            theme.colorScheme.secondary,
          );
        } else {
          expect(find.byKey(const ValueKey('sankey-plot')), findsOneWidget);
        }
        if (['line', 'area', 'bar', 'candlestick'].contains(type)) {
          final AxisChartData data = switch (type) {
            'bar' => tester.widget<BarChart>(find.byType(BarChart)).data,
            'candlestick' =>
              tester
                  .widget<CandlestickChart>(find.byType(CandlestickChart))
                  .data,
            _ => tester.widget<LineChart>(find.byType(LineChart)).data,
          };
          expect(data.gridData.drawHorizontalLine, isTrue, reason: type);
          expect(data.gridData.drawVerticalLine, isTrue, reason: type);
          expect(
            data.gridData.getDrawingHorizontalLine(0).color,
            theme.colorScheme.outlineVariant,
            reason: type,
          );
          expect(
            data.gridData.getDrawingVerticalLine(0).color,
            theme.colorScheme.outlineVariant,
            reason: type,
          );
        }
        final semantics = tester
            .widgetList<Semantics>(find.byType(Semantics))
            .map((widget) => widget.properties.label)
            .whereType<String>();
        final description = switch (type) {
          'sankey' => 'Flow: a → b: 2.0',
          'candlestick' =>
            'Prices: Mon: 2.0 / 4.0 / 1.0 / 3.0; Tue: 3.0 / 5.0 / 2.0 / 2.0',
          _ => 'Values: A: 1.0; B: 2.0; C: 3.0',
        };
        expect(semantics, contains(description), reason: type);
        expect(tester.takeException(), isNull, reason: type);
      }
    });
  }

  testWidgets('invalid source stays selectable and updates recover', (
    tester,
  ) async {
    const invalid = '{"type":"unknown","data":[]}';
    await mount(tester, invalid);
    expect(tester.widget<CodeBlock>(find.byType(CodeBlock)).text, invalid);
    expect(find.byType(SelectableText), findsOneWidget);
    await mount(tester, jsonEncode(points('line')));
    expect(find.byType(LineChart), findsOneWidget);
    expect(find.byType(CodeBlock), findsNothing);
    await mount(tester, invalid);
    expect(find.byType(LineChart), findsNothing);
    expect(find.byType(CodeBlock), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('touch details retain full pie and radar values', (tester) async {
    await mount(tester, jsonEncode(points('pie')));
    final pie = tester.widget<PieChart>(find.byType(PieChart)).data;
    pie.pieTouchData.touchCallback!(
      FlTapDownEvent(TapDownDetails()),
      PieTouchResponse(
        touchLocation: Offset.zero,
        touchedSection: PieTouchedSection(pie.sections[1], 1, 0, 0),
      ),
    );
    await tester.pump();
    expect(find.text('B: 2'), findsOneWidget);
    await mount(tester, jsonEncode(points('radar')));
    final radar = tester.widget<RadarChart>(find.byType(RadarChart)).data;
    radar.radarTouchData.touchCallback!(
      FlTapDownEvent(TapDownDetails()),
      RadarTouchResponse(
        touchLocation: Offset.zero,
        touchedSpot: RadarTouchedSpot(
          radar.dataSets.first,
          0,
          radar.dataSets.first.dataEntries[2],
          2,
          const FlSpot(0, 3),
          Offset.zero,
        ),
      ),
    );
    await tester.pump();
    expect(find.text('C: 3'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('Sankey bands expose touch details', (tester) async {
    await mount(tester, jsonEncode(graph()));
    final plot = find.byKey(const ValueKey('sankey-plot'));
    await tester.tapAt(tester.getCenter(plot));
    await tester.pump();
    expect(find.text('A → B: 2'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('tiny Sankey strokes retain hit targets', (tester) async {
    await mount(
      tester,
      jsonEncode({
        ...graph(),
        'links': [
          {'source': 'a', 'target': 'b', 'value': 5e-324},
          {'source': 'a', 'target': 'b', 'value': 1e12},
        ],
      }),
    );
    final plot = find.byKey(const ValueKey('sankey-plot'));
    final rect = tester.getRect(plot);
    await tester.tapAt(Offset(rect.center.dx, rect.top + 22));
    await tester.pump();
    expect(find.text('A → B: 5e-324'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('negative and constant axes remain finite', (tester) async {
    for (final type in ['line', 'bar', 'area']) {
      for (final values in [
        <num>[0],
        <num>[-1e12],
        <num>[-2, 0, 3],
      ]) {
        await mount(
          tester,
          jsonEncode({
            'type': type,
            'data': [
              for (final value in values) {'label': '', 'value': value},
            ],
          }),
        );
        final data = type == 'bar'
            ? tester.widget<BarChart>(find.byType(BarChart)).data
            : tester.widget<LineChart>(find.byType(LineChart)).data;
        expect(data.minY.isFinite, isTrue);
        expect(data.maxY.isFinite, isTrue);
        expect(data.minY, lessThan(data.maxY));
        expect(tester.takeException(), isNull);
      }
    }
    await mount(
      tester,
      jsonEncode({
        'type': 'candlestick',
        'data': [
          {'label': '', 'open': 0, 'high': 0, 'low': 0, 'close': 0},
        ],
      }),
    );
    final data = tester
        .widget<CandlestickChart>(find.byType(CandlestickChart))
        .data;
    expect(data.minY, lessThan(data.maxY));
    expect(tester.takeException(), isNull);
  });
}
