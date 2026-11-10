import 'package:flutter/material.dart';

import '../../../ui/theme.dart';
import 'metric_charts.dart';
import 'metric_history.dart';

const figures = [FontFeature.tabularFigures()];

TextStyle dimStyle(BuildContext context, [double size = 12]) => TextStyle(
  fontSize: size,
  height: 1.3,
  color: chartDim(context),
  fontFeatures: figures,
);

TextStyle mutedStyle(BuildContext context, [double size = 12]) => TextStyle(
  fontSize: size,
  height: 1.3,
  color: Theme.of(context).colorScheme.onSurfaceVariant,
  fontFeatures: figures,
);

const titleStyle = TextStyle(
  fontSize: 15,
  height: 1.3,
  fontWeight: FontWeight.w600,
  fontFeatures: figures,
);

/// Borderless nested card inside a glass Surface.
class InsetCard extends StatelessWidget {
  const InsetCard({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
  });
  final Widget child;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) => DecoratedBox(
    decoration: BoxDecoration(
      color: SailryTheme.insetFill(context),
      borderRadius: BorderRadius.circular(14),
    ),
    child: Padding(padding: padding, child: child),
  );
}

/// Card title, optional headline value and a dim trailing detail.
class CardHeader extends StatelessWidget {
  const CardHeader({super.key, required this.title, this.value, this.detail});
  final String title;
  final String? value;
  final String? detail;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(bottom: 8),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.baseline,
      textBaseline: TextBaseline.alphabetic,
      children: [
        Text(title, style: titleStyle),
        if (value != null) ...[
          const SizedBox(width: 8),
          Text(value!, style: titleStyle),
        ],
        const SizedBox(width: 12),
        Expanded(
          child: Text(
            detail ?? '',
            textAlign: TextAlign.end,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: dimStyle(context),
          ),
        ),
      ],
    ),
  );
}

class LegendMark extends StatelessWidget {
  const LegendMark(this.color, {super.key});
  final Color color;

  @override
  Widget build(BuildContext context) => Container(
    width: 8,
    height: 3,
    decoration: BoxDecoration(
      color: color,
      borderRadius: BorderRadius.circular(2),
    ),
  );
}

/// Legend entry: colour mark, muted label and an emphasized value.
class LegendEntry extends StatelessWidget {
  const LegendEntry({
    super.key,
    required this.color,
    required this.label,
    required this.value,
  });
  final Color color;
  final String label;
  final String value;

  @override
  Widget build(BuildContext context) => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      LegendMark(color),
      const SizedBox(width: 6),
      Text('$label ', style: mutedStyle(context)),
      Text(
        value,
        style: mutedStyle(context).copyWith(
          color: Theme.of(context).colorScheme.onSurface,
          fontWeight: FontWeight.w500,
        ),
      ),
    ],
  );
}

/// Current transfer rate with its legend mark, without an inner fill.
class RateValue extends StatelessWidget {
  const RateValue({
    super.key,
    required this.color,
    required this.label,
    required this.bytes,
  });
  final Color color;
  final String label;
  final double? bytes;

  @override
  Widget build(BuildContext context) {
    final parts = bytes == null ? null : byteParts(bytes!);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            LegendMark(color),
            const SizedBox(width: 6),
            Text(label, style: dimStyle(context)),
          ],
        ),
        const SizedBox(height: 2),
        Text.rich(
          TextSpan(
            text: parts?.$1 ?? '—',
            children: [
              if (parts != null)
                TextSpan(
                  text: ' ${parts.$2}/s',
                  style: dimStyle(context).copyWith(height: 1),
                ),
            ],
          ),
          style: titleStyle,
        ),
      ],
    );
  }
}
