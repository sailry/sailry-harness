import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../ui/project_icon.dart';
import '../../../ui/theme.dart';
import 'metric_history.dart';

enum MetricTone { cpu, memory, gpu, read, write, received, sent }

/// Chart colours from the theme and the shared GPUI Kit named scales.
Color metricColor(BuildContext context, MetricTone tone) {
  final theme = Theme.of(context);
  Color scale(String name) {
    final (dark, light) = ProjectIcon.colors[name]!;
    return Color(theme.brightness == Brightness.dark ? dark : light);
  }

  return switch (tone) {
    MetricTone.cpu => SailryTheme.accentColor(context, AccentTone.blue),
    MetricTone.memory => scale('violet'),
    MetricTone.gpu => scale('magenta'),
    MetricTone.read => scale('teal'),
    MetricTone.write => scale('amber'),
    MetricTone.received => theme.colorScheme.tertiary,
    MetricTone.sent => theme.colorScheme.secondary,
  };
}

/// Replaces a series colour once a share crosses the warning thresholds.
Color levelColor(BuildContext context, double share, Color normal) =>
    switch (meterLevel(share)) {
      MeterLevel.normal => normal,
      MeterLevel.warning => SailryTheme.statusColor(
        context,
        StatusTone.warning,
      ),
      MeterLevel.danger => SailryTheme.statusColor(context, StatusTone.danger),
    };

Color chartTrack(BuildContext context) =>
    Theme.of(context).colorScheme.onSurfaceVariant.withValues(alpha: .12);

Color chartDim(BuildContext context) =>
    Theme.of(context).colorScheme.onSurfaceVariant.withValues(alpha: .65);

class ChartSeries {
  const ChartSeries(this.values, this.color);
  final List<double?> values;
  final Color color;
}

/// Overlaid area series with optional dashed half and full gridlines.
class AreaChart extends StatelessWidget {
  const AreaChart({
    super.key,
    required this.series,
    required this.ceiling,
    required this.height,
    this.gridLabels,
    this.axisLabels,
    this.slots = historyCapacity,
  });

  final List<ChartSeries> series;
  final double ceiling;
  final double height;

  /// Half and full gridline labels; null draws no grid.
  final (String, String)? gridLabels;

  /// Oldest and newest x-axis labels.
  final (String, String)? axisLabels;

  /// Horizontal positions; newer samples align to the right edge.
  final int slots;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      height: height,
      width: double.infinity,
      child: CustomPaint(
        painter: _AreaPainter(
          series: series,
          ceiling: ceiling,
          slots: slots,
          gridLabels: gridLabels,
          axisLabels: axisLabels,
          grid: colors.outlineVariant,
          label: chartDim(context),
          direction: Directionality.of(context),
        ),
      ),
    );
  }
}

class _AreaPainter extends CustomPainter {
  _AreaPainter({
    required this.series,
    required this.ceiling,
    required this.slots,
    required this.gridLabels,
    required this.axisLabels,
    required this.grid,
    required this.label,
    required this.direction,
  });

  final List<ChartSeries> series;
  final double ceiling;
  final int slots;
  final (String, String)? gridLabels;
  final (String, String)? axisLabels;
  final Color grid;
  final Color label;
  final TextDirection direction;

  static const _axisHeight = 16.0;

  TextPainter _text(String value) => TextPainter(
    text: TextSpan(
      text: value,
      style: TextStyle(fontSize: 9, color: label),
    ),
    textDirection: direction,
  )..layout();

  @override
  void paint(Canvas canvas, Size size) {
    final bottom = axisLabels == null ? size.height : size.height - _axisHeight;
    final top = gridLabels == null ? 0.0 : 1.0;
    final chart = bottom - top;
    if (gridLabels case (final half, final full)) {
      final dash = Paint()
        ..color = grid
        ..strokeWidth = 1;
      for (final (fraction, value) in [(.5, half), (1.0, full)]) {
        final y = bottom - chart * fraction;
        for (var x = 0.0; x < size.width; x += 6) {
          canvas.drawLine(
            Offset(x, y),
            Offset(math.min(x + 3, size.width), y),
            dash,
          );
        }
        final text = _text(value);
        text.paint(canvas, Offset(size.width - text.width, y + 2));
      }
      canvas.drawLine(Offset(0, bottom), Offset(size.width, bottom), dash);
    }
    if (axisLabels case (final start, final end)) {
      final first = _text(start);
      first.paint(canvas, Offset(0, bottom + 4));
      final last = _text(end);
      last.paint(canvas, Offset(size.width - last.width, bottom + 4));
    }
    if (ceiling <= 0 || slots < 2) return;
    for (final item in series) {
      final values = item.values.length > slots
          ? item.values.sublist(item.values.length - slots)
          : item.values;
      final offset = slots - values.length;
      // Missing samples split the series instead of drawing through a gap.
      final runs = <List<Offset>>[[]];
      for (final (index, value) in values.indexed) {
        if (value == null) {
          if (runs.last.isNotEmpty) runs.add([]);
          continue;
        }
        final x = size.width * (index + offset) / (slots - 1);
        final y = bottom - chart * (value / ceiling).clamp(0, 1);
        runs.last.add(Offset(x, y));
      }
      for (final run in runs.where((run) => run.isNotEmpty)) {
        final line = Path()..moveTo(run.first.dx, run.first.dy);
        for (final point in run.skip(1)) {
          line.lineTo(point.dx, point.dy);
        }
        final area = Path.from(line)
          ..lineTo(run.last.dx, bottom)
          ..lineTo(run.first.dx, bottom)
          ..close();
        canvas.drawPath(
          area,
          Paint()
            ..shader = LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [
                item.color.withValues(alpha: .26),
                item.color.withValues(alpha: 0),
              ],
            ).createShader(Rect.fromLTRB(0, top, size.width, bottom)),
        );
        if (run.length == 1) {
          canvas.drawCircle(run.first, 1.5, Paint()..color = item.color);
          continue;
        }
        canvas.drawPath(
          line,
          Paint()
            ..color = item.color
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1.5
            ..strokeJoin = StrokeJoin.round,
        );
      }
    }
  }

  @override
  bool shouldRepaint(_AreaPainter old) => true;
}

class RingSegment {
  const RingSegment(this.share, this.color);
  final double share;
  final Color color;
}

/// Circular usage ring starting at twelve o'clock.
class RingChart extends StatelessWidget {
  const RingChart({
    super.key,
    required this.segments,
    required this.child,
    this.size = 96,
  });

  final List<RingSegment> segments;
  final Widget child;
  final double size;

  @override
  Widget build(BuildContext context) => SizedBox.square(
    dimension: size,
    child: CustomPaint(
      painter: _RingPainter(segments, chartTrack(context)),
      child: Center(child: child),
    ),
  );
}

class _RingPainter extends CustomPainter {
  _RingPainter(this.segments, this.track);
  final List<RingSegment> segments;
  final Color track;

  static const _stroke = 9.0;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Rect.fromCircle(
      center: size.center(Offset.zero),
      radius: (size.shortestSide - _stroke) / 2,
    );
    final paint = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = _stroke;
    canvas.drawArc(rect, 0, math.pi * 2, false, paint..color = track);
    var start = -math.pi / 2;
    final gap = segments.length > 1 ? .03 : 0.0;
    for (final segment in segments) {
      final sweep = math.pi * 2 * segment.share.clamp(0, 1);
      if (sweep <= gap) {
        start += sweep;
        continue;
      }
      canvas.drawArc(
        rect,
        start,
        sweep - gap,
        false,
        paint..color = segment.color,
      );
      start += sweep;
    }
  }

  @override
  bool shouldRepaint(_RingPainter old) => true;
}

/// Thin rounded progress bar coloured by its threshold level.
class MeterBar extends StatelessWidget {
  const MeterBar({super.key, required this.share, required this.color});
  final double share;
  final Color color;

  @override
  Widget build(BuildContext context) => ClipRRect(
    borderRadius: BorderRadius.circular(2),
    child: SizedBox(
      height: 4,
      child: Stack(
        fit: StackFit.expand,
        children: [
          ColoredBox(color: chartTrack(context)),
          FractionallySizedBox(
            alignment: AlignmentDirectional.centerStart,
            widthFactor: share.clamp(0, 1),
            child: ColoredBox(color: levelColor(context, share, color)),
          ),
        ],
      ),
    ),
  );
}

/// Per-core utilization cells filling from the bottom; hold one for its value.
class CoreGrid extends StatelessWidget {
  const CoreGrid({super.key, required this.shares});
  final List<double> shares;

  static const _columns = 12;

  @override
  Widget build(BuildContext context) {
    final cpu = metricColor(context, MetricTone.cpu);
    final track = Theme.of(
      context,
    ).colorScheme.onSurfaceVariant.withValues(alpha: .06);
    return LayoutBuilder(
      builder: (context, constraints) {
        const spacing = 3.0;
        final width =
            (constraints.maxWidth - spacing * (_columns - 1)) / _columns;
        return Wrap(
          spacing: spacing,
          runSpacing: 4,
          children: [
            for (final share in shares)
              Tooltip(
                message: percent(share),
                triggerMode: TooltipTriggerMode.longPress,
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(3),
                  child: SizedBox(
                    width: width,
                    height: 26,
                    child: ColoredBox(
                      color: track,
                      child: FractionallySizedBox(
                        alignment: Alignment.bottomCenter,
                        heightFactor: share.clamp(0, 1),
                        widthFactor: 1,
                        child: ColoredBox(
                          color: levelColor(
                            context,
                            share,
                            cpu,
                          ).withValues(alpha: .85),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
          ],
        );
      },
    );
  }
}
