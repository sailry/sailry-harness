import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
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

/// A directory entry on two lines: its name, then the kind or the file's
/// type, size and modification time. A tile before it tells folders from
/// files at a glance: a tinted folder, or the file's extension.
class FileEntryRow extends StatelessWidget {
  const FileEntryRow({
    super.key,
    required this.name,
    required this.directory,
    required this.size,
    required this.onTap,
    this.modified,
  });

  final String name;
  final bool directory;

  /// Formatted size, shown for files only.
  final String size;

  /// Formatted modification time; null when the host did not say.
  final String? modified;
  final VoidCallback? onTap;

  /// The extension a tile shows: up to four letters, none for a dotfile or a
  /// name without one.
  static String? extension(String name) {
    final dot = name.lastIndexOf('.');
    if (dot <= 0 || dot == name.length - 1) return null;
    final value = name.substring(dot + 1);
    return value.length > 4 ? null : value.toUpperCase();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final dim = colors.onSurfaceVariant.withValues(alpha: .75);
    final blue = SailryTheme.accentColor(context, AccentTone.blue);
    final kind = extension(name);
    final details = directory
        ? [context.tr('folder'), ?modified]
        : [?kind, size, ?modified];
    final tile = SizedBox.square(
      dimension: 38,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: directory
              ? blue.withValues(alpha: .16)
              : colors.surfaceContainerHigh,
          borderRadius: BorderRadius.circular(10),
        ),
        child: Center(
          child: directory || kind == null
              ? AppIcon(
                  directory ? 'folder' : 'file',
                  size: 20,
                  color: directory ? blue : colors.onSurfaceVariant,
                )
              : Text(
                  kind,
                  maxLines: 1,
                  style: TextStyle(
                    fontSize: kind.length > 3 ? 9 : 10.5,
                    fontWeight: FontWeight.w700,
                    letterSpacing: .2,
                    color: colors.onSurfaceVariant,
                  ),
                ),
        ),
      ),
    );
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(12),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 9),
        child: Row(
          children: [
            tile,
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                spacing: 2,
                children: [
                  Text(
                    name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  Text(
                    details.join(' · '),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12.5,
                      color: dim,
                      fontFeatures: const [FontFeature.tabularFigures()],
                    ),
                  ),
                ],
              ),
            ),
            if (directory) ...[
              const SizedBox(width: 8),
              AppIcon('chevron', size: 14, color: dim),
            ],
          ],
        ),
      ),
    );
  }
}
