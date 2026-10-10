import 'dart:math' as math;

import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';

import 'data.dart';
import 'style.dart';

class RadialPlot extends StatefulWidget {
  const RadialPlot({super.key, required this.spec});
  final ChartSpec spec;

  @override
  State<RadialPlot> createState() => _RadialPlotState();
}

class _RadialPlotState extends State<RadialPlot> {
  int? _selected;

  @override
  void didUpdateWidget(RadialPlot oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.spec != widget.spec) _selected = null;
  }

  void _select(FlTouchEvent event, int? index) {
    if (!event.isInterestedForInteractions) index = null;
    if (index != null && (index < 0 || index >= widget.spec.points.length)) {
      index = null;
    }
    if (index != _selected) setState(() => _selected = index);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final spec = widget.spec;
      final theme = Theme.of(context);
      final colors = theme.colorScheme;
      final palette = plotColors(context);
      final labelStyle = theme.textTheme.bodySmall?.copyWith(
        fontSize: 12,
        color: colors.onSurface,
      );
      final radius = math.max(
        1.0,
        math.min(constraints.maxWidth, constraints.maxHeight) / 2 - 44,
      );
      final Widget chart;
      if (spec.type == ChartType.pie) {
        chart = PieChart(
          PieChartData(
            centerSpaceRadius: 0,
            sectionsSpace: 0,
            sections: [
              for (final (index, point) in spec.points.indexed)
                PieChartSectionData(
                  value: point.value,
                  color: palette[index % palette.length],
                  radius: radius + (_selected == index ? 4 : 0),
                  title: plotLabel(point.label),
                  showTitle: spec.points.length <= 12 && point.value > 0,
                  titleStyle: labelStyle,
                  titlePositionPercentageOffset: 1.18,
                ),
            ],
            pieTouchData: PieTouchData(
              touchCallback: (event, response) =>
                  _select(event, response?.touchedSection?.touchedSectionIndex),
            ),
          ),
          duration: Duration.zero,
        );
      } else {
        chart = Padding(
          padding: const EdgeInsets.all(36),
          child: RadarChart(
            RadarChartData(
              radarShape: RadarShape.polygon,
              radarBorderData: BorderSide(color: colors.outlineVariant),
              gridBorderData: BorderSide(color: colors.outlineVariant),
              tickBorderData: BorderSide(color: colors.outlineVariant),
              ticksTextStyle: labelStyle,
              tickCount: 3,
              isMinValueAtCenter: true,
              titleTextStyle: labelStyle,
              getTitle: (index, _) => RadarChartTitle(
                text: spec.points.length <= 24
                    ? plotLabel(spec.points[index].label)
                    : '',
              ),
              dataSets: [
                RadarDataSet(
                  dataEntries: [
                    for (final point in spec.points)
                      RadarEntry(value: point.value),
                  ],
                  fillColor: palette.first.withValues(alpha: .2),
                  borderColor: palette.first,
                  entryRadius: spec.points.length <= 40 ? 3 : 0,
                ),
                // A transparent zero dataset anchors the radial scale at zero.
                RadarDataSet(
                  dataEntries: [
                    for (final _ in spec.points) const RadarEntry(value: 0),
                  ],
                  fillColor: Colors.transparent,
                  borderColor: Colors.transparent,
                  borderWidth: 0,
                  entryRadius: 0,
                ),
              ],
              radarTouchData: RadarTouchData(
                touchCallback: (event, response) => _select(
                  event,
                  response?.touchedSpot?.touchedDataSetIndex == 0
                      ? response?.touchedSpot?.touchedRadarEntryIndex
                      : null,
                ),
              ),
            ),
            duration: Duration.zero,
          ),
        );
      }
      return Stack(
        fit: StackFit.expand,
        children: [
          chart,
          if (_selected case final index?)
            PlotDetail(
              '${spec.points[index].label}: ${plotNumber(spec.points[index].value)}',
            ),
        ],
      );
    },
  );
}
