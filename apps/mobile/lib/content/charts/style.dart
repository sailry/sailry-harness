import 'package:flutter/material.dart';

import '../../ui/theme.dart';

List<Color> plotColors(BuildContext context) {
  final colors = Theme.of(context).colorScheme;
  return [
    colors.secondary,
    colors.tertiary,
    SailryTheme.accentColor(context, AccentTone.blue),
    SailryTheme.accentColor(context, AccentTone.purple),
    SailryTheme.accentColor(context, AccentTone.yellow),
  ];
}

String plotNumber(double value) => value == value.roundToDouble()
    ? value.toStringAsFixed(0)
    : value.toString();

/// Rendered labels stay compact; tooltips and semantics retain the full label.
String plotLabel(String value) {
  final runes = value.runes.toList();
  return runes.length <= 12
      ? value
      : '${String.fromCharCodes(runes.take(11))}…';
}

class PlotDetail extends StatelessWidget {
  const PlotDetail(this.text, {super.key});
  final String text;

  @override
  Widget build(BuildContext context) => IgnorePointer(
    child: Align(
      alignment: Alignment.bottomCenter,
      child: Container(
        margin: const EdgeInsets.all(8),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.inverseSurface,
          borderRadius: BorderRadius.circular(8),
        ),
        child: Text(
          text,
          style: Theme.of(context).textTheme.bodySmall?.copyWith(
            color: Theme.of(context).colorScheme.onInverseSurface,
          ),
        ),
      ),
    ),
  );
}
