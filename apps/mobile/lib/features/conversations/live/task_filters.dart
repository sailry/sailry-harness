import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/pill_tabs.dart';

/// Status filters shown as tabs; archive and terminal views are reached elsewhere.
const taskFilters = ['all', 'waiting', 'running', 'completed'];

/// Status tabs sized to their labels, with the archive entry at the end.
class TaskFilters extends StatelessWidget {
  const TaskFilters({
    super.key,
    required this.selected,
    required this.counts,
    required this.archived,
    required this.onSelected,
    required this.onArchive,
  });

  final String selected;
  final Map<String, int> counts;
  final int archived;
  final ValueChanged<String> onSelected;
  final VoidCallback onArchive;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Row(
      children: [
        Expanded(
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: PillTabs<String>(
              tabs: [
                for (final filter in taskFilters)
                  (
                    value: filter,
                    label: context.tr(filter),
                    count: filter == 'all' || (counts[filter] ?? 0) == 0
                        ? null
                        : counts[filter],
                  ),
              ],
              selected: selected,
              onSelected: onSelected,
            ),
          ),
        ),
        if (archived > 0)
          TextButton(
            key: const ValueKey('task-archive'),
            style: TextButton.styleFrom(
              foregroundColor: colors.onSurfaceVariant,
              padding: const EdgeInsets.symmetric(horizontal: 12),
              minimumSize: const Size(0, 36),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              shape: const StadiumBorder(),
            ),
            onPressed: onArchive,
            child: Semantics(
              label: context.tr('archiveTab'),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                spacing: 4,
                children: [
                  AppIcon('archive', size: 16, color: colors.onSurfaceVariant),
                  Text(
                    '$archived',
                    style: const TextStyle(
                      fontSize: 12,
                      fontFeatures: [FontFeature.tabularFigures()],
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

/// Header for the archive and terminal views, which have no tab of their own.
class TaskScope extends StatelessWidget {
  const TaskScope({super.key, required this.label, required this.onClose});

  final String label;
  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) => Row(
    children: [
      Expanded(
        child: Text(
          label,
          style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
        ),
      ),
      TextButton(onPressed: onClose, child: Text(context.tr('back'))),
    ],
  );
}
