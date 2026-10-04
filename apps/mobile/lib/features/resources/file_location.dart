import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';

/// Worktree-relative breadcrumbs keep navigation inside the selected worktree.
class FileLocation extends StatelessWidget {
  const FileLocation({
    super.key,
    required this.directory,
    required this.onNavigate,
    this.enabled = true,
  });

  final String directory;
  final ValueChanged<String> onNavigate;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final parts = directory.isEmpty ? <String>[] : directory.split('/');
    Widget crumb(String label, String path, {bool root = false}) {
      final current = path == directory;
      return Semantics(
        selected: current,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 160),
          child: TextButton(
            onPressed: enabled && !current ? () => onNavigate(path) : null,
            style: TextButton.styleFrom(
              foregroundColor: colors.onSurfaceVariant,
              disabledForegroundColor: current ? colors.onSurface : null,
              backgroundColor: current ? colors.surfaceContainerHighest : null,
              padding: const EdgeInsets.symmetric(horizontal: 10),
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(10),
              ),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (root) ...[
                  const AppIcon('folder', size: 16),
                  const SizedBox(width: 6),
                ],
                Flexible(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          ),
        ),
      );
    }

    return Surface(
      radius: 16,
      padding: const EdgeInsets.all(4),
      child: Row(
        children: [
          Expanded(
            child: LayoutBuilder(
              builder: (context, constraints) => SingleChildScrollView(
                key: ValueKey(directory),
                scrollDirection: Axis.horizontal,
                reverse: true,
                child: ConstrainedBox(
                  constraints: BoxConstraints(minWidth: constraints.maxWidth),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      crumb(tr('resourceRoot'), '', root: true),
                      for (var index = 0; index < parts.length; index++) ...[
                        const Padding(
                          padding: EdgeInsets.symmetric(horizontal: 2),
                          child: AppIcon('chevron', size: 12),
                        ),
                        crumb(parts[index], parts.take(index + 1).join('/')),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
