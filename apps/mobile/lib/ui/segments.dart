import 'package:flutter/material.dart';

import 'icons.dart';
import 'theme.dart';

/// A choice among a few equal segments, after Desktop's segmented tabs: a
/// muted track whose chosen segment is inverted (the text color behind the
/// surface's) on an indicator that slides to it, the others muted. Segments
/// show [icon]s with [label]s as their tooltips, or the labels themselves
/// when there are no icons. [expand] fills the width; otherwise each
/// segment is [segmentWidth] wide.
class Segments<T> extends StatelessWidget {
  const Segments({
    super.key,
    required this.values,
    required this.selected,
    required this.label,
    required this.onChanged,
    this.icon,
    this.expand = false,
    this.segmentWidth = 36,
  });

  final List<T> values;
  final T selected;
  final String Function(T value) label;
  final String Function(T value)? icon;
  final ValueChanged<T>? onChanged;
  final bool expand;
  final double segmentWidth;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final n = values.length;
    final index = values.indexOf(selected);
    final height = icon == null ? 30.0 : 28.0;
    final track = Container(
      width: expand ? null : n * segmentWidth + 6,
      height: height + 6,
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: colors.onSurface.withValues(alpha: .07),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Stack(
        children: [
          if (index >= 0)
            AnimatedAlign(
              alignment: Alignment(n == 1 ? 0 : -1 + 2 * index / (n - 1), 0),
              duration: const Duration(milliseconds: 240),
              curve: Curves.easeOutCubic,
              child: FractionallySizedBox(
                widthFactor: 1 / n,
                heightFactor: 1,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: SailryTheme.brand(context).color,
                    borderRadius: BorderRadius.circular(7),
                  ),
                ),
              ),
            ),
          Row(
            children: [
              for (final value in values)
                Expanded(child: _segment(context, value, value == selected)),
            ],
          ),
        ],
      ),
    );
    return Opacity(opacity: onChanged == null ? .5 : 1, child: track);
  }

  Widget _segment(BuildContext context, T value, bool chosen) {
    final colors = Theme.of(context).colorScheme;
    final brand = SailryTheme.brand(context).color;
    // The chosen segment's text and icon read on the brand fill: light on
    // a deep brand color, dark on a pale one.
    final onBrand =
        ThemeData.estimateBrightnessForColor(brand) == Brightness.dark
        ? Colors.white
        : const Color(0xff1c1b17);
    final color = chosen ? onBrand : colors.onSurfaceVariant;
    final text = label(value);
    final content = icon == null
        ? AnimatedDefaultTextStyle(
            duration: const Duration(milliseconds: 160),
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w500,
              color: color,
            ),
            child: Text(text, maxLines: 1, softWrap: false),
          )
        : AppIcon(icon!(value), size: 16, color: color);
    Widget segment = Semantics(
      button: true,
      selected: chosen,
      label: text,
      excludeSemantics: true,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: onChanged == null || chosen ? null : () => onChanged!(value),
        child: Center(child: content),
      ),
    );
    if (icon != null) segment = Tooltip(message: text, child: segment);
    return segment;
  }
}
