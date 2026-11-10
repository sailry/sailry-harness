import 'package:flutter/material.dart';

/// One tab of [PillTabs], with an optional dim count after its label.
typedef PillTab<T> = ({T value, String label, int? count});

/// Filter-style tabs matching the conversation list filters.
class PillTabs<T> extends StatelessWidget {
  const PillTabs({
    super.key,
    required this.tabs,
    required this.selected,
    required this.onSelected,
    this.expand = false,
  });

  final List<PillTab<T>> tabs;
  final T selected;
  final ValueChanged<T>? onSelected;

  /// Share the full width equally instead of sizing tabs to their labels.
  final bool expand;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    Widget tab(PillTab<T> item) {
      final active = item.value == selected;
      return Semantics(
        selected: active,
        child: TextButton(
          key: ValueKey('pill-tab-${item.value}'),
          style: TextButton.styleFrom(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
            minimumSize: const Size(0, 36),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            backgroundColor: active
                ? colors.surfaceContainerHigh
                : Colors.transparent,
            disabledBackgroundColor: active
                ? colors.surfaceContainerHigh
                : Colors.transparent,
            foregroundColor: active
                ? colors.onSurface
                : colors.onSurfaceVariant,
            shape: const StadiumBorder(),
          ),
          onPressed: onSelected == null
              ? null
              : () {
                  if (!active) onSelected!(item.value);
                },
          child: Text.rich(
            TextSpan(
              text: item.label,
              children: [
                if (item.count != null)
                  TextSpan(
                    text: '  ${item.count}',
                    style: TextStyle(
                      fontSize: 12,
                      color: colors.onSurfaceVariant.withValues(alpha: .65),
                      fontFeatures: const [FontFeature.tabularFigures()],
                    ),
                  ),
              ],
            ),
            softWrap: false,
            style: TextStyle(
              color: active ? colors.onSurface : colors.onSurfaceVariant,
              fontWeight: active ? FontWeight.w500 : null,
            ),
          ),
        ),
      );
    }

    return Row(
      spacing: 4,
      children: [
        for (final item in tabs)
          if (expand) Expanded(child: tab(item)) else tab(item),
      ],
    );
  }
}
