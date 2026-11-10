import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'data.dart';
import 'style.dart';

/// fl_chart has no Sankey component; this native painter consumes the same DAG.
class SankeyPlot extends StatefulWidget {
  const SankeyPlot({super.key, required this.spec});
  final ChartSpec spec;

  @override
  State<SankeyPlot> createState() => _SankeyPlotState();
}

class _SankeyPlotState extends State<SankeyPlot> {
  String? _detail;

  @override
  void didUpdateWidget(SankeyPlot oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.spec != oldWidget.spec) _detail = null;
  }

  void _select(Offset position, _Layout layout) {
    final detail = layout.detail(position);
    if (detail != _detail) setState(() => _detail = detail);
  }

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final layout = _Layout(widget.spec, constraints.biggest);
      return Stack(
        fit: StackFit.expand,
        children: [
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: SingleChildScrollView(
              child: MouseRegion(
                onHover: (event) => _select(event.localPosition, layout),
                onExit: (_) {
                  if (_detail != null) setState(() => _detail = null);
                },
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTapUp: (event) => _select(event.localPosition, layout),
                  child: CustomPaint(
                    key: const ValueKey('sankey-plot'),
                    size: layout.size,
                    painter: _Painter(
                      layout,
                      plotColors(context),
                      Theme.of(context).textTheme.bodySmall!.copyWith(
                        fontSize: 12,
                        color: Theme.of(context).colorScheme.onSurface,
                      ),
                      Directionality.of(context),
                    ),
                  ),
                ),
              ),
            ),
          ),
          if (_detail case final detail?) PlotDetail(detail),
        ],
      );
    },
  );
}

class _Layout {
  _Layout(this.spec, Size viewport) {
    final ids = {
      for (final (index, node) in spec.nodes.indexed) node.id: index,
    };
    final edges = List.generate(spec.nodes.length, (_) => <int>[]);
    final degrees = List.filled(spec.nodes.length, 0);
    final depths = List.filled(spec.nodes.length, 0);
    final incoming = List.filled(spec.nodes.length, 0.0);
    final outgoing = List.filled(spec.nodes.length, 0.0);
    for (final link in spec.links) {
      final source = ids[link.source]!;
      final target = ids[link.target]!;
      edges[source].add(target);
      degrees[target]++;
      outgoing[source] += link.value;
      incoming[target] += link.value;
    }
    final ready = [
      for (var index = 0; index < degrees.length; index++)
        if (degrees[index] == 0) index,
    ];
    while (ready.isNotEmpty) {
      final node = ready.removeLast();
      for (final target in edges[node]) {
        depths[target] = math.max(depths[target], depths[node] + 1);
        if (--degrees[target] == 0) ready.add(target);
      }
    }
    final columns = List.generate(depths.reduce(math.max) + 1, (_) => <int>[]);
    for (final (index, depth) in depths.indexed) {
      columns[depth].add(index);
    }
    size = Size(
      math.max(viewport.width, columns.length * 120.0),
      math.max(
        viewport.height,
        columns.map((column) => column.length).reduce(math.max) * 24.0 + 32,
      ),
    );
    values = [
      for (var index = 0; index < spec.nodes.length; index++)
        math.max(incoming[index], outgoing[index]),
    ];
    final maximum = values.reduce(math.max);
    final weights = values.map((value) => value / maximum).toList();
    var scale = double.infinity;
    for (final column in columns) {
      final total = column.fold(0.0, (sum, index) => sum + weights[index]);
      if (total > 0) {
        scale = math.min(
          scale,
          (size.height - 32 - column.length * 12 - (column.length - 1) * 12) /
              total,
        );
      }
    }
    rects = List.filled(spec.nodes.length, Rect.zero);
    for (final (depth, column) in columns.indexed) {
      final heights = [for (final index in column) 12 + weights[index] * scale];
      var top =
          (size.height -
              heights.fold(0.0, (sum, value) => sum + value) -
              (column.length - 1) * 12) /
          2;
      final left = 16 + depth * (size.width - 48) / (columns.length - 1);
      for (final (position, index) in column.indexed) {
        rects[index] = Rect.fromLTWH(left, top, 16, heights[position]);
        top += heights[position] + 12;
      }
    }
    final sourceOffsets = List.filled(spec.nodes.length, 6.0);
    final targetOffsets = List.filled(spec.nodes.length, 6.0);
    bands = [];
    centers = [];
    for (final link in spec.links) {
      final source = ids[link.source]!;
      final target = ids[link.target]!;
      final height = link.value / maximum * scale;
      final start = Offset(
        rects[source].right,
        rects[source].top + sourceOffsets[source],
      );
      final end = Offset(
        rects[target].left,
        rects[target].top + targetOffsets[target],
      );
      final middle = (start.dx + end.dx) / 2;
      bands.add(
        Path()
          ..moveTo(start.dx, start.dy)
          ..cubicTo(middle, start.dy, middle, end.dy, end.dx, end.dy)
          ..lineTo(end.dx, end.dy + height)
          ..cubicTo(
            middle,
            end.dy + height,
            middle,
            start.dy + height,
            start.dx,
            start.dy + height,
          )
          ..close(),
      );
      centers.add((
        start: start + Offset(0, height / 2),
        end: end + Offset(0, height / 2),
        width: height,
        source: source,
      ));
      sourceOffsets[source] += height;
      targetOffsets[target] += height;
    }
  }

  final ChartSpec spec;
  late final Size size;
  late final List<double> values;
  late final List<Rect> rects;
  late final List<Path> bands;
  late final List<({Offset start, Offset end, double width, int source})>
  centers;

  String? detail(Offset position) {
    for (final (index, rect) in rects.indexed) {
      if (rect.inflate(4).contains(position)) {
        return '${spec.nodes[index].label}: ${plotNumber(values[index])}';
      }
    }
    for (final (index, band) in bands.indexed) {
      if (band.contains(position) || _nearCurve(position, centers[index])) {
        final link = spec.links[index];
        final source = spec.nodes.firstWhere((node) => node.id == link.source);
        final target = spec.nodes.firstWhere((node) => node.id == link.target);
        return '${source.label} → ${target.label}: ${plotNumber(link.value)}';
      }
    }
    return null;
  }
}

bool _nearCurve(
  Offset position,
  ({Offset start, Offset end, double width, int source}) curve,
) {
  if (curve.width >= 8 ||
      !Rect.fromPoints(curve.start, curve.end).inflate(4).contains(position)) {
    return false;
  }
  final middle = (curve.start.dx + curve.end.dx) / 2;
  var previous = curve.start;
  for (var index = 1; index <= 64; index++) {
    final time = index / 64;
    final remaining = 1 - time;
    final point =
        curve.start * (remaining * remaining * remaining) +
        Offset(middle, curve.start.dy) * (3 * remaining * remaining * time) +
        Offset(middle, curve.end.dy) * (3 * remaining * time * time) +
        curve.end * (time * time * time);
    final segment = point - previous;
    final length = segment.distanceSquared;
    final relative = position - previous;
    final fraction = length == 0
        ? 0.0
        : ((relative.dx * segment.dx + relative.dy * segment.dy) / length)
              .clamp(0.0, 1.0);
    if ((position - (previous + segment * fraction)).distanceSquared <= 16) {
      return true;
    }
    previous = point;
  }
  return false;
}

class _Painter extends CustomPainter {
  _Painter(this.layout, this.colors, this.style, this.direction);
  final _Layout layout;
  final List<Color> colors;
  final TextStyle style;
  final TextDirection direction;

  @override
  void paint(Canvas canvas, Size size) {
    for (final (index, band) in layout.bands.indexed) {
      final center = layout.centers[index];
      final color = colors[center.source % colors.length];
      canvas.drawPath(band, Paint()..color = color.withValues(alpha: .3));
      if (center.width < 1) {
        final middle = (center.start.dx + center.end.dx) / 2;
        canvas.drawPath(
          Path()
            ..moveTo(center.start.dx, center.start.dy)
            ..cubicTo(
              middle,
              center.start.dy,
              middle,
              center.end.dy,
              center.end.dx,
              center.end.dy,
            ),
          Paint()
            ..color = color.withValues(alpha: .5)
            ..style = PaintingStyle.stroke
            ..strokeWidth = 1,
        );
      }
    }
    for (final (index, rect) in layout.rects.indexed) {
      canvas.drawRRect(
        RRect.fromRectAndRadius(rect, const Radius.circular(2)),
        Paint()..color = colors[index % colors.length],
      );
      final label = TextPainter(
        text: TextSpan(text: layout.spec.nodes[index].label, style: style),
        textDirection: direction,
        maxLines: 1,
        ellipsis: '…',
      )..layout(maxWidth: 90);
      final last = rect.right > size.width - 24;
      label.paint(
        canvas,
        Offset(
          last ? rect.left - label.width - 6 : rect.right + 6,
          rect.center.dy - label.height / 2,
        ),
      );
    }
  }

  @override
  bool shouldRepaint(_Painter oldDelegate) =>
      oldDelegate.layout != layout ||
      oldDelegate.style != style ||
      oldDelegate.direction != direction ||
      !listEquals(oldDelegate.colors, colors);
}
