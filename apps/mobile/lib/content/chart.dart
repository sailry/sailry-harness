import 'package:flutter/material.dart';

import 'charts/axes.dart';
import 'charts/data.dart';
import 'charts/radial.dart';
import 'charts/sankey.dart';
import 'code.dart';

/// Native chart fences share Desktop's closed, bounded JSON schema.
class SailryChart extends StatefulWidget {
  const SailryChart({super.key, required this.source});

  final String source;

  @override
  State<SailryChart> createState() => _SailryChartState();
}

class _SailryChartState extends State<SailryChart> {
  ChartSpec? _spec;

  @override
  void initState() {
    super.initState();
    _spec = ChartSpec.parse(widget.source);
  }

  @override
  void didUpdateWidget(SailryChart oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.source != oldWidget.source) {
      _spec = ChartSpec.parse(widget.source);
    }
  }

  @override
  Widget build(BuildContext context) {
    final spec = _spec;
    if (spec == null) return CodeBlock(widget.source, language: 'chart');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (spec.title.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Text(
              spec.title,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ),
        Semantics(
          label: spec.title.isEmpty
              ? spec.description
              : '${spec.title}: ${spec.description}',
          child: SizedBox(
            key: ValueKey('chart-${spec.type.name}'),
            height: 300,
            child: switch (spec.type) {
              ChartType.pie || ChartType.radar => RadialPlot(spec: spec),
              ChartType.sankey => SankeyPlot(spec: spec),
              _ => AxisPlot(spec: spec),
            },
          ),
        ),
      ],
    );
  }
}
