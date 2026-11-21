import 'package:flutter/material.dart';

import '../../../ui/kit.dart';
import 'tool_row.dart' show transcriptFontSize;

/// Pill above the composer in the same glass material as cards.
class GlassChip extends StatelessWidget {
  const GlassChip({super.key, required this.child, this.onTap});

  final Widget child;

  /// Null leaves the chip inert and dimmed.
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Opacity(
      opacity: onTap == null ? .6 : 1,
      child: SizedBox(
        height: 32,
        child: Surface(
          radius: 16,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          onTap: onTap,
          child: Center(
            widthFactor: 1,
            child: DefaultTextStyle.merge(
              maxLines: 1,
              softWrap: false,
              style: TextStyle(
                fontSize: transcriptFontSize,
                color: colors.onSurfaceVariant,
              ),
              child: child,
            ),
          ),
        ),
      ),
    );
  }
}

/// The single line of chips above the composer. Narrow screens scroll it
/// horizontally instead of wrapping, and chips start at the leading edge.
class ComposerChipRow extends StatelessWidget {
  const ComposerChipRow({super.key, required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Align(
    alignment: AlignmentDirectional.centerStart,
    child: SingleChildScrollView(
      key: const ValueKey('composer-chips'),
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.fromLTRB(12, 0, 12, 2),
      child: Row(spacing: 8, children: children),
    ),
  );
}
