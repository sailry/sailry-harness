import 'dart:math' as math;
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import 'live.dart';
import 'usage_data.dart';

const _figures = [FontFeature.tabularFigures()];

/// Visible breakdown rows before the list is expanded.
const _visibleRows = 5;

/// Series colours for cached input, uncached input and output.
List<Color> usageColors(BuildContext context) {
  final colors = Theme.of(context).colorScheme;
  return [
    colors.tertiary,
    SailryTheme.accentColor(context, AccentTone.blue),
    colors.secondary,
  ];
}

const _series = [
  'settingsUsageCache',
  'settingsUsageInput',
  'settingsUsageOutput',
];

Color _dim(BuildContext context) =>
    Theme.of(context).colorScheme.onSurfaceVariant.withValues(alpha: .65);

TextStyle _muted(BuildContext context, double size) => TextStyle(
  fontSize: size,
  height: 1.3,
  color: Theme.of(context).colorScheme.onSurfaceVariant,
  fontFeatures: _figures,
);

TextStyle _faint(BuildContext context, double size) => TextStyle(
  fontSize: size,
  height: 1.3,
  color: _dim(context),
  fontFeatures: _figures,
);

/// Plain-fill segmented choice shared by the range and breakdown controls.
class UsageSegments<T> extends StatelessWidget {
  const UsageSegments({
    super.key,
    required this.values,
    required this.selected,
    required this.label,
    required this.onChanged,
  });
  final List<T> values;
  final T selected;
  final String Function(T value) label;
  final ValueChanged<T> onChanged;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SegmentedButton<T>(
      expandedInsets: EdgeInsets.zero,
      showSelectedIcon: false,
      segments: [
        for (final value in values)
          ButtonSegment(
            value: value,
            label: Text(label(value), maxLines: 1, softWrap: false),
          ),
      ],
      selected: {selected},
      onSelectionChanged: (selection) => onChanged(selection.single),
      style: SegmentedButton.styleFrom(
        side: BorderSide.none,
        backgroundColor: colors.surfaceContainer,
        foregroundColor: colors.onSurfaceVariant,
        selectedBackgroundColor: colors.surfaceContainerHigh,
        selectedForegroundColor: colors.onSurface,
        textStyle: const TextStyle(fontSize: 13),
        visualDensity: VisualDensity.compact,
        padding: const EdgeInsets.symmetric(horizontal: 4),
      ),
    );
  }
}

/// Token total, composition, metric grid and the partial-data note.
class UsageSummary extends StatelessWidget {
  const UsageSummary({super.key, required this.overview, required this.stale});
  final UsageOverview overview;

  /// Some hosts have not reported current data.
  final bool stale;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final unknown = context.tr('settingsUsageUnknown');
    final series = usageColors(context);
    final tokens = overview.tokens;
    final shares = overview.shares;
    Widget part(Color color, String name, int? value, double? share) => Row(
      children: [
        Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(
            color: color,
            borderRadius: BorderRadius.circular(2),
          ),
        ),
        const SizedBox(width: 6),
        Flexible(
          child: Text(
            name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: _muted(context, 13),
          ),
        ),
        const SizedBox(width: 6),
        Text(
          compact(value) ?? unknown,
          style: const TextStyle(
            fontSize: 13,
            fontWeight: FontWeight.w500,
            fontFeatures: _figures,
          ),
        ),
        if (share != null) ...[
          const SizedBox(width: 6),
          Text(percent(share)!, style: _faint(context, 13)),
        ],
      ],
    );
    final parts = [
      for (var index = 0; index < 3; index++)
        part(
          series[index],
          context.tr(_series[index]),
          tokens?.parts[index],
          shares[index],
        ),
      part(
        _dim(context),
        context.tr('usageReasoning'),
        overview.reasoning,
        null,
      ),
    ];
    final priced = context
        .tr('usageCoverage')
        .replaceAll('{priced}', grouped(overview.priced)!)
        .replaceAll('{total}', grouped(overview.responses)!);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(context.tr('tokens'), style: _muted(context, 13)),
        const SizedBox(height: 2),
        Row(
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          children: [
            Text(
              compact(overview.total) ?? unknown,
              key: const ValueKey('usage-total'),
              style: const TextStyle(
                fontSize: 40,
                height: 1.15,
                fontWeight: FontWeight.w700,
                letterSpacing: -.5,
                fontFeatures: _figures,
              ),
            ),
            if (overview.total != null) ...[
              const SizedBox(width: 6),
              Flexible(
                child: Text(
                  grouped(overview.total)!,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: _faint(context, 15),
                ),
              ),
            ],
          ],
        ),
        const SizedBox(height: 12),
        _Composition(parts: tokens?.parts, colors: series),
        const SizedBox(height: 12),
        for (final row in [parts.sublist(0, 2), parts.sublist(2)])
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Row(
              children: [
                Expanded(child: row[0]),
                const SizedBox(width: 12),
                Expanded(child: row[1]),
              ],
            ),
          ),
        const SizedBox(height: 10),
        DecoratedBox(
          decoration: BoxDecoration(
            border: Border.all(color: colors.outlineVariant),
            borderRadius: BorderRadius.circular(16),
          ),
          child: Column(
            children: [
              _MetricRow(
                left: _Metric(
                  label: context.tr('usageHitRate'),
                  value: overview.hit == null
                      ? unknown
                      : '${overview.hit!.round()}%',
                  footer: Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: LinearProgressIndicator(
                      value: (overview.hit ?? 0) / 100,
                      minHeight: 4,
                      borderRadius: BorderRadius.circular(2),
                      color: series[0],
                      backgroundColor: colors.surfaceContainerHigh,
                    ),
                  ),
                ),
                right: _Metric(
                  label: context.tr('requests'),
                  value: grouped(overview.responses)!,
                ),
              ),
              const Divider(),
              _MetricRow(
                left: _Metric(
                  label: context.tr('usageSpeed'),
                  value: overview.speed ?? unknown,
                  extra: overview.speed == null
                      ? null
                      : Text(
                          context.tr('usageSpeedUnit'),
                          style: _faint(context, 12),
                        ),
                ),
                right: _Metric(
                  label: context.tr('estimatedCost'),
                  value: overview.cost == null ? unknown : '\$${overview.cost}',
                  extra: overview.partial
                      ? _PartialTag(
                          onTap: () => showAppSheet(
                            context,
                            context.tr('estimatedCost'),
                            child: Text(priced, style: _muted(context, 14)),
                          ),
                        )
                      : null,
                ),
              ),
            ],
          ),
        ),
        if (stale) ...[
          const SizedBox(height: 8),
          Text(
            context.tr('settingsUsagePartial'),
            key: const ValueKey('usage-partial'),
            style: _faint(context, 12),
          ),
        ],
      ],
    );
  }
}

class _Composition extends StatelessWidget {
  const _Composition({required this.parts, required this.colors});
  final List<int>? parts;
  final List<Color> colors;
  @override
  Widget build(BuildContext context) {
    final values = parts ?? const [0, 0, 0];
    final sum = values.fold<int>(0, (sum, value) => sum + value);
    return ClipRRect(
      borderRadius: BorderRadius.circular(4),
      child: Container(
        height: 8,
        color: Theme.of(context).colorScheme.surfaceContainerHigh,
        child: sum == 0
            ? null
            : Row(
                spacing: 2,
                children: [
                  for (var index = 0; index < 3; index++)
                    if (values[index] > 0)
                      Expanded(
                        // Flex needs integers; keep every nonzero part visible.
                        flex: math.max(1, values[index] * 1000 ~/ sum),
                        child: ColoredBox(color: colors[index]),
                      ),
                ],
              ),
      ),
    );
  }
}

class _MetricRow extends StatelessWidget {
  const _MetricRow({required this.left, required this.right});
  final Widget left;
  final Widget right;
  @override
  Widget build(BuildContext context) => IntrinsicHeight(
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(child: left),
        const VerticalDivider(width: 1),
        Expanded(child: right),
      ],
    ),
  );
}

class _Metric extends StatelessWidget {
  const _Metric({
    required this.label,
    required this.value,
    this.extra,
    this.footer,
  });
  final String label;
  final String value;
  final Widget? extra;
  final Widget? footer;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(14, 14, 14, 12),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: _muted(context, 12),
        ),
        const SizedBox(height: 4),
        Row(
          crossAxisAlignment: CrossAxisAlignment.baseline,
          textBaseline: TextBaseline.alphabetic,
          children: [
            Flexible(
              child: Text(
                value,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  fontSize: 20,
                  height: 1.3,
                  fontWeight: FontWeight.w600,
                  fontFeatures: _figures,
                ),
              ),
            ),
            if (extra != null) ...[const SizedBox(width: 5), extra!],
          ],
        ),
        ?footer,
      ],
    ),
  );
}

class _PartialTag extends StatelessWidget {
  const _PartialTag({required this.onTap});
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) => Material(
    color: Theme.of(context).colorScheme.surfaceContainerHigh,
    shape: const StadiumBorder(),
    clipBehavior: Clip.antiAlias,
    child: InkWell(
      key: const ValueKey('usage-cost-partial'),
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        child: Text(
          context.tr('usageCostPartial'),
          style: _muted(context, 11).copyWith(fontWeight: FontWeight.w500),
        ),
      ),
    ),
  );
}

/// Section title with an optional trailing control.
class UsageHeading extends StatelessWidget {
  const UsageHeading(this.title, {super.key, this.trailing});
  final String title;
  final Widget? trailing;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: 10),
    child: Row(
      children: [
        Text(
          title,
          style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
        ),
        if (trailing != null) ...[
          const SizedBox(width: 12),
          Expanded(
            child: Align(alignment: Alignment.centerRight, child: trailing),
          ),
        ],
      ],
    ),
  );
}

/// Stacked token trend, the yearly heatmap and the local-time footnote.
class UsageTrend extends StatelessWidget {
  const UsageTrend({
    super.key,
    required this.report,
    required this.range,
    required this.offset,
  });
  final Map<String, dynamic> report;
  final UsageRange range;

  /// Offset in minutes the report was bucketed at.
  final int offset;
  @override
  Widget build(BuildContext context) {
    final series = usageColors(context);
    final unit = range.unit;
    final points = objects(
      unit == UsageUnit.halfHour ? report['recent'] : report['days'],
    );
    final buckets = usageBuckets(points, unit, offset);
    final year = range == UsageRange.year
        ? usageHeatmap(objects(report['days']), offset)
        : null;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        UsageHeading(
          context.tr('usageTrend'),
          trailing: Wrap(
            spacing: 12,
            alignment: WrapAlignment.end,
            children: [
              for (var index = 0; index < 3; index++)
                Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                        color: series[index],
                        borderRadius: BorderRadius.circular(2),
                      ),
                    ),
                    const SizedBox(width: 5),
                    Text(
                      context.tr(_series[index]),
                      style: _muted(context, 12),
                    ),
                  ],
                ),
            ],
          ),
        ),
        _TrendChart(buckets: buckets, unit: unit, colors: series),
        if (year != null) ...[
          const SizedBox(height: 14),
          Text(
            context
                .tr('usageActiveDays')
                .replaceAll('{count}', '${year.active}'),
            style: _faint(context, 12),
          ),
          const SizedBox(height: 8),
          _Heatmap(columns: year.columns, cells: year.cells, color: series[0]),
        ],
        const SizedBox(height: 8),
        Text(
          context.tr('usageLocalTime'),
          textAlign: TextAlign.right,
          style: _faint(context, 12),
        ),
      ],
    );
  }
}

class _TrendChart extends StatelessWidget {
  const _TrendChart({
    required this.buckets,
    required this.unit,
    required this.colors,
  });
  final List<UsageBucket> buckets;
  final UsageUnit unit;
  final List<Color> colors;
  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final unknown = context.tr('settingsUsageUnknown');
    final labels = usageLabels(buckets, unit);
    final axis = usageAxis(
      buckets.fold<int>(
        0,
        (peak, bucket) => math.max(peak, bucket.tokens?.total ?? 0),
      ),
    );
    final tick = _faint(context, 11);
    return SizedBox(
      height: 170,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final slot =
              (constraints.maxWidth - 36) / math.max(1, buckets.length);
          return BarChart(
            BarChartData(
              minY: 0,
              maxY: axis.maximum,
              alignment: BarChartAlignment.spaceAround,
              borderData: FlBorderData(
                show: true,
                border: Border(
                  bottom: BorderSide(color: scheme.outlineVariant),
                ),
              ),
              gridData: FlGridData(
                drawVerticalLine: false,
                horizontalInterval: axis.step,
                getDrawingHorizontalLine: (_) => FlLine(
                  color: scheme.outlineVariant,
                  strokeWidth: 1,
                  dashArray: const [3, 3],
                ),
              ),
              barTouchData: BarTouchData(
                allowTouchBarBackDraw: true,
                touchTooltipData: BarTouchTooltipData(
                  fitInsideHorizontally: true,
                  fitInsideVertically: true,
                  maxContentWidth: 220,
                  tooltipBorderRadius: BorderRadius.circular(10),
                  tooltipPadding: const EdgeInsets.symmetric(
                    horizontal: 10,
                    vertical: 8,
                  ),
                  tooltipBorder: BorderSide(color: scheme.outlineVariant),
                  getTooltipColor: (_) => Color.alphaBlend(
                    scheme.surfaceContainerHigh,
                    scheme.surface,
                  ),
                  getTooltipItem: (group, _, _, _) {
                    final bucket = buckets[group.x];
                    final tokens = bucket.tokens;
                    return BarTooltipItem(
                      usageHeading(bucket, unit),
                      TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w500,
                        color: scheme.onSurface,
                        fontFeatures: _figures,
                      ),
                      textAlign: TextAlign.left,
                      children: [
                        for (var index = 0; index < 3; index++) ...[
                          TextSpan(
                            text: '\n■ ',
                            style: TextStyle(color: colors[index]),
                          ),
                          TextSpan(
                            text:
                                '${context.tr(_series[index])} ${tokens == null ? unknown : compact(tokens.parts[index])}',
                            style: TextStyle(
                              fontWeight: FontWeight.w400,
                              color: scheme.onSurface,
                            ),
                          ),
                        ],
                      ],
                    );
                  },
                ),
              ),
              titlesData: FlTitlesData(
                leftTitles: const AxisTitles(),
                topTitles: const AxisTitles(),
                rightTitles: AxisTitles(
                  sideTitles: SideTitles(
                    showTitles: true,
                    reservedSize: 36,
                    interval: axis.step,
                    getTitlesWidget: (value, meta) => value == 0
                        ? const SizedBox.shrink()
                        : Padding(
                            padding: const EdgeInsets.only(left: 6),
                            child: Text(
                              value < 1000
                                  ? '${value.round()}'
                                  : compact(
                                      value.round(),
                                    )!.replaceAll('.0', ''),
                              style: tick,
                            ),
                          ),
                  ),
                ),
                bottomTitles: AxisTitles(
                  sideTitles: SideTitles(
                    showTitles: true,
                    reservedSize: 22,
                    getTitlesWidget: (value, meta) {
                      final index = value.toInt();
                      if (index < 0 ||
                          index >= labels.length ||
                          labels[index].isEmpty) {
                        return const SizedBox.shrink();
                      }
                      return Padding(
                        padding: const EdgeInsets.only(top: 6),
                        child: Text(
                          labels[index],
                          softWrap: false,
                          overflow: TextOverflow.visible,
                          style: tick,
                        ),
                      );
                    },
                  ),
                ),
              ),
              barGroups: [
                for (var index = 0; index < buckets.length; index++)
                  _group(index, buckets[index].tokens, slot, axis.maximum),
              ],
            ),
            duration: const Duration(milliseconds: 220),
          );
        },
      ),
    );
  }

  BarChartGroupData _group(
    int index,
    TokenStack? tokens,
    double slot,
    double maximum,
  ) {
    final stops = <double>[0];
    for (final value in tokens?.parts ?? const <int>[]) {
      stops.add(stops.last + value);
    }
    return BarChartGroupData(
      x: index,
      barRods: [
        BarChartRodData(
          toY: tokens?.total.toDouble() ?? 0,
          width: math.max(2, math.min(18, slot * .62)),
          color: Colors.transparent,
          borderRadius: const BorderRadius.vertical(top: Radius.circular(3)),
          // Transparent full-height backgrounds make empty buckets touchable.
          backDrawRodData: BackgroundBarChartRodData(
            show: true,
            toY: maximum,
            color: Colors.transparent,
          ),
          rodStackItems: [
            if (tokens != null)
              for (var part = 0; part < 3; part++)
                BarChartRodStackItem(
                  stops[part],
                  stops[part + 1],
                  colors[part],
                ),
          ],
        ),
      ],
    );
  }
}

class _Heatmap extends StatelessWidget {
  const _Heatmap({
    required this.columns,
    required this.cells,
    required this.color,
  });
  final int columns;
  final List<UsageCell> cells;
  final Color color;
  @override
  Widget build(BuildContext context) => AspectRatio(
    aspectRatio: columns / 7,
    child: CustomPaint(
      painter: _HeatmapPainter(
        columns: columns,
        cells: cells,
        color: color,
        empty: Theme.of(context).colorScheme.surfaceContainerHigh,
      ),
    ),
  );
}

class _HeatmapPainter extends CustomPainter {
  _HeatmapPainter({
    required this.columns,
    required this.cells,
    required this.color,
    required this.empty,
  });
  final int columns;
  final List<UsageCell> cells;
  final Color color;
  final Color empty;
  @override
  void paint(Canvas canvas, Size size) {
    const gap = 1.5;
    final cell = math.min(
      (size.width - gap * (columns - 1)) / columns,
      (size.height - gap * 6) / 7,
    );
    if (cell <= 0) return;
    final paint = Paint();
    for (final item in cells) {
      paint.color = item.intensity == 0
          ? empty
          : color.withValues(alpha: .2 + .8 * item.intensity);
      canvas.drawRRect(
        RRect.fromRectAndRadius(
          Rect.fromLTWH(
            item.column * (cell + gap),
            item.row * (cell + gap),
            cell,
            cell,
          ),
          const Radius.circular(2),
        ),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(_HeatmapPainter old) =>
      old.cells != cells ||
      old.columns != columns ||
      old.color != color ||
      old.empty != empty;
}

/// Breakdown list with a five-row preview.
class UsageRows extends StatelessWidget {
  const UsageRows({
    super.key,
    required this.rows,
    required this.expanded,
    required this.onToggle,
  });
  final List<UsageRow> rows;
  final bool expanded;
  final VoidCallback onToggle;
  @override
  Widget build(BuildContext context) {
    final unknown = context.tr('settingsUsageUnknown');
    final colors = Theme.of(context).colorScheme;
    final shown = expanded ? rows : rows.take(_visibleRows).toList();
    if (rows.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 24),
        child: Text(
          context.tr('usageEmpty'),
          textAlign: TextAlign.center,
          style: _faint(context, 13),
        ),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (var index = 0; index < shown.length; index++) ...[
          if (index > 0) const Divider(),
          Padding(
            key: ValueKey('usage-row-${shown[index].id}'),
            padding: const EdgeInsets.symmetric(vertical: 11),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Expanded(
                      child: Text.rich(
                        TextSpan(
                          text: shown[index].name,
                          children: [
                            if (shown[index].note case final note?)
                              TextSpan(
                                text: '  $note',
                                style: _faint(context, 13),
                              ),
                          ],
                        ),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      compact(shown[index].total) ?? unknown,
                      style: const TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w500,
                        fontFeatures: _figures,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 7),
                LinearProgressIndicator(
                  value: math.min(shown[index].share ?? 0, 100) / 100,
                  minHeight: 4,
                  borderRadius: BorderRadius.circular(2),
                  color: colors.onSurfaceVariant,
                  backgroundColor: colors.surfaceContainerHigh,
                ),
                const SizedBox(height: 6),
                Wrap(
                  spacing: 14,
                  children: [
                    Text(
                      percent(shown[index].share) ?? unknown,
                      style: _muted(context, 12),
                    ),
                    Text(
                      context
                          .tr('usageHit')
                          .replaceAll(
                            '{value}',
                            shown[index].hit == null
                                ? unknown
                                : '${shown[index].hit!.round()}%',
                          ),
                      style: _faint(context, 12),
                    ),
                    Text(
                      context
                          .tr('usageResponseCount')
                          .replaceAll(
                            '{count}',
                            grouped(shown[index].responses)!,
                          ),
                      style: _faint(context, 12),
                    ),
                    Text(
                      shown[index].cost == null
                          ? unknown
                          : '\$${shown[index].cost}',
                      style: _faint(context, 12),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
        if (rows.length > _visibleRows)
          Center(
            child: TextButton(
              key: const ValueKey('usage-rows-toggle'),
              style: TextButton.styleFrom(
                foregroundColor: colors.onSurfaceVariant,
              ),
              onPressed: onToggle,
              child: Text(
                context.tr(expanded ? 'usageCollapse' : 'usageShowAll'),
              ),
            ),
          ),
      ],
    );
  }
}
