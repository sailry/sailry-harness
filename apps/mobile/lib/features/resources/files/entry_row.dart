import 'package:flutter/material.dart';

import '../../../ui/kit.dart';
import '../../../ui/theme.dart';

/// Search-field styled button that opens the full-text search flow.
class SearchBarButton extends StatelessWidget {
  const SearchBarButton({super.key, required this.label, required this.onTap});
  final String label;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .65);
    return Semantics(
      button: true,
      label: label,
      excludeSemantics: true,
      child: Material(
        color: colors.surfaceContainerHigh,
        shape: const StadiumBorder(),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: SizedBox(
            height: 40,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14),
              child: Row(
                children: [
                  AppIcon('search', size: 16, color: dim),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 14, color: dim),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Flat directory row; the hairline starts after the icon.
class FileEntryRow extends StatelessWidget {
  const FileEntryRow({
    super.key,
    required this.name,
    required this.directory,
    required this.size,
    required this.onTap,
    this.divider = true,
  });

  final String name;
  final bool directory;

  /// Formatted size, shown for files only.
  final String size;
  final VoidCallback? onTap;
  final bool divider;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .65);
    final folder = Color.lerp(
      colors.onSurfaceVariant,
      SailryTheme.accentColor(context, AccentTone.blue),
      .4,
    )!;
    return InkWell(
      onTap: onTap,
      child: Row(
        children: [
          AppIcon(
            directory ? 'folder' : 'file',
            size: 19,
            color: directory ? folder : colors.onSurfaceVariant,
          ),
          const SizedBox(width: 12),
          Expanded(
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: divider
                    ? Border(bottom: BorderSide(color: colors.outlineVariant))
                    : null,
              ),
              child: Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: Row(
                  children: [
                    Expanded(
                      child: Text(
                        name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 14),
                      ),
                    ),
                    const SizedBox(width: 8),
                    if (directory)
                      AppIcon('chevron', size: 14, color: dim)
                    else
                      Text(
                        size,
                        style: TextStyle(
                          fontSize: 12,
                          color: dim,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
