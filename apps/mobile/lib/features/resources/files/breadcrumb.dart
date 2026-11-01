import 'package:flutter/material.dart';

import '../../../ui/kit.dart';

/// Worktree-relative ancestors of [directory], root first.
List<({String label, String path})> crumbs(String directory) {
  final parts = directory.split('/').where((part) => part.isNotEmpty).toList();
  return [
    (label: '', path: ''),
    for (var index = 0; index < parts.length; index++)
      (label: parts[index], path: parts.take(index + 1).join('/')),
  ];
}

/// Scrollable path pills that keep navigation inside the selected worktree.
class Breadcrumb extends StatelessWidget {
  const Breadcrumb({
    super.key,
    required this.directory,
    required this.root,
    required this.onNavigate,
    this.enabled = true,
  });

  final String directory;

  /// Label of the worktree root pill.
  final String root;
  final ValueChanged<String> onNavigate;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final items = crumbs(directory);
    Widget pill(({String label, String path}) item, {required bool first}) {
      final current = item.path == items.last.path;
      final label = first ? root : item.label;
      return Semantics(
        selected: current,
        button: !current,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 180),
          child: Material(
            color: current
                ? colors.surfaceContainerHigh
                : colors.onSurface.withValues(alpha: .04),
            shape: const StadiumBorder(),
            clipBehavior: Clip.antiAlias,
            child: InkWell(
              onTap: enabled && !current ? () => onNavigate(item.path) : null,
              child: Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: 10,
                  vertical: 5,
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    if (first) ...[
                      AppIcon(
                        'folder',
                        size: 13,
                        color: current
                            ? colors.onSurface
                            : colors.onSurfaceVariant,
                      ),
                      const SizedBox(width: 5),
                    ],
                    Flexible(
                      child: Text(
                        label,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 13,
                          color: current
                              ? colors.onSurface
                              : colors.onSurfaceVariant,
                          fontWeight: current ? FontWeight.w500 : null,
                        ),
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

    return LayoutBuilder(
      builder: (context, constraints) => SingleChildScrollView(
        key: ValueKey(directory),
        scrollDirection: Axis.horizontal,
        // Keep the current directory in view.
        reverse: true,
        child: ConstrainedBox(
          constraints: BoxConstraints(minWidth: constraints.maxWidth),
          child: Row(
            spacing: 4,
            children: [
              for (final (index, item) in items.indexed)
                pill(item, first: index == 0),
            ],
          ),
        ),
      ),
    );
  }
}
