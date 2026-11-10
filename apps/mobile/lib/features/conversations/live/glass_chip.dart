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
