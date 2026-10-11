import 'dart:math' as math;

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import 'data.dart';
import 'style.dart';

class AxisPlot extends StatelessWidget {
  const AxisPlot({super.key, required this.spec});
  final ChartSpec spec;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final count = spec.type == ChartType.candlestick
          ? spec.candles.length
          : spec.points.length;
      final width =
          spec.type == ChartType.bar || spec.type == ChartType.candlestick
          ? math.max(constraints.maxWidth, count * 24.0)
          : constraints.maxWidth;
      return SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: SizedBox(
          width: width,
          height: constraints.maxHeight,
          child: Padding(
            padding: const EdgeInsets.only(top: 12, right: 12, bottom: 4),
            child: _chart(context, width),
          ),
        ),
      );
    },
  );

  Widget _chart(BuildContext context, double width) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final color = plotColors(context).first;
    final labels = spec.type == ChartType.candlestick
        ? spec.candles.map((candle) => candle.label).toList()
        : spec.points.map((point) => point.label).toList();
    final titleStyle = theme.textTheme.bodySmall?.copyWith(
      fontSize: 11,
      color: colors.onSurfaceVariant,
    );
    final titles = FlTitlesData(
      topTitles: const AxisTitles(),
      rightTitles: const AxisTitles(),
      leftTitles: AxisTitles(
        sideTitles: SideTitles(
          showTitles: true,
          reservedSize: 64,
          getTitlesWidget: (value, _) => Text(
            value.abs() >= 1e6 || value != 0 && value.abs() < .001
                ? value.toStringAsExponential(1)
                : plotNumber(double.parse(value.toStringAsPrecision(4))),
            style: titleStyle,
            textAlign: TextAlign.right,
          ),
        ),
      ),
      bottomTitles: AxisTitles(
        sideTitles: SideTitles(
          showTitles: true,
          reservedSize: 32,
          interval: math
              .max(1, (labels.length / math.max(2, width / 72)).ceil())
              .toDouble(),
          getTitlesWidget: (value, _) {
            final index = value.round();
            if ((value - index).abs() > .001 ||
                index < 0 ||
                index >= labels.length) {
              return const SizedBox.shrink();
            }
            return Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Tooltip(
                message: labels[index],
                child: Text(plotLabel(labels[index]), style: titleStyle),
              ),
            );
          },
        ),
      ),
    );
    final grid = FlGridData(
      getDrawingHorizontalLine: (_) =>
          FlLine(color: colors.outlineVariant, strokeWidth: 1),
      getDrawingVerticalLine: (_) =>
          FlLine(color: colors.outlineVariant, strokeWidth: 1),
    );
    final border = FlBorderData(show: false);
    final values = spec.type == ChartType.candlestick
        ? spec.candles.expand((candle) => [candle.low, candle.high])
        : spec.points.map((point) => point.value);
    var low = values.reduce(math.min);
    var high = values.reduce(math.max);
    if (spec.type == ChartType.bar || spec.type == ChartType.area) {
      low = math.min(0, low);
      high = math.max(0, high);
    }
    var padding = (high - low) * .08;
    if (padding == 0) padding = high.abs() * .08;
    if (padding == 0) padding = 1;
    final tooltipStyle = theme.textTheme.bodySmall!.copyWith(
      color: colors.onInverseSurface,
    );
    if (spec.type == ChartType.candlestick) {
      return CandlestickChart(
        CandlestickChartData(
          minX: -.5,
          maxX: labels.length - .5,
          minY: low - padding,
          maxY: high + padding,
          titlesData: titles,
          gridData: grid,
          borderData: border,
          candlestickSpots: [
            for (final (index, candle) in spec.candles.indexed)
              CandlestickSpot(
                x: index.toDouble(),
                open: candle.open,
                high: candle.high,
                low: candle.low,
                close: candle.close,
              ),
          ],
          candlestickPainter: DefaultCandlestickPainter(
            candlestickStyleProvider: (spot, _) {
              final tone = spot.close >= spot.open
                  ? colors.tertiary
                  : colors.secondary;
              return CandlestickStyle(
                lineColor: tone,
                lineWidth: 1,
                bodyStrokeColor: tone,
                bodyStrokeWidth: 1,
                bodyFillColor: tone.withValues(alpha: .55),
                bodyWidth: 12,
                bodyRadius: 2,
              );
            },
          ),
          candlestickTouchData: CandlestickTouchData(
            touchTooltipData: CandlestickTouchTooltipData(
              fitInsideHorizontally: true,
              fitInsideVertically: true,
              getTooltipColor: (_) => colors.inverseSurface,
              getTooltipItems: (_, spot, index) => CandlestickTooltipItem(
                '${labels[index]}: ${plotNumber(spot.open)} / ${plotNumber(spot.high)} / ${plotNumber(spot.low)} / ${plotNumber(spot.close)}',
                textStyle: tooltipStyle,
              ),
            ),
          ),
        ),
        duration: Duration.zero,
      );
    }
    if (spec.type == ChartType.bar) {
      return BarChart(
        BarChartData(
          minY: low - padding,
          maxY: high + padding,
          titlesData: titles,
          gridData: grid,
          borderData: border,
          barGroups: [
            for (final (index, point) in spec.points.indexed)
              BarChartGroupData(
                x: index,
                barRods: [
                  BarChartRodData(
                    toY: point.value,
                    color: color,
                    width: 14,
                    borderRadius: const BorderRadius.all(Radius.circular(2)),
                  ),
                ],
              ),
          ],
          barTouchData: BarTouchData(
            touchTooltipData: BarTouchTooltipData(
              fitInsideHorizontally: true,
              fitInsideVertically: true,
              getTooltipColor: (_) => colors.inverseSurface,
              getTooltipItem: (group, _, rod, _) => BarTooltipItem(
                '${labels[group.x]}: ${plotNumber(rod.toY)}',
                tooltipStyle,
              ),
            ),
          ),
        ),
        duration: Duration.zero,
      );
    }
    return LineChart(
      LineChartData(
        minX: labels.length == 1 ? -.5 : 0,
        maxX: labels.length == 1 ? .5 : (labels.length - 1).toDouble(),
        minY: low - padding,
        maxY: high + padding,
        titlesData: titles,
        gridData: grid,
        borderData: border,
        lineBarsData: [
          LineChartBarData(
            color: color,
            spots: [
              for (final (index, point) in spec.points.indexed)
                FlSpot(index.toDouble(), point.value),
            ],
            dotData: FlDotData(show: spec.points.length <= 40),
            belowBarData: BarAreaData(
              show: spec.type == ChartType.area,
              color: color.withValues(alpha: .2),
              applyCutOffY: true,
            ),
            aboveBarData: BarAreaData(
              show: spec.type == ChartType.area,
              color: color.withValues(alpha: .2),
              applyCutOffY: true,
            ),
          ),
        ],
        lineTouchData: LineTouchData(
          touchTooltipData: LineTouchTooltipData(
            fitInsideHorizontally: true,
            fitInsideVertically: true,
            getTooltipColor: (_) => colors.inverseSurface,
            getTooltipItems: (spots) => [
              for (final spot in spots)
                LineTooltipItem(
                  '${labels[spot.spotIndex]}: ${plotNumber(spot.y)}',
                  tooltipStyle,
                ),
            ],
          ),
        ),
      ),
      duration: Duration.zero,
    );
  }
}
