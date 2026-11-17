import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/pill_tabs.dart';

/// Status filters shown as tabs; archive and terminal views are reached elsewhere.
const taskFilters = ['all', 'waiting', 'running', 'completed'];

/// Status tabs sized to their labels, with the archive button always at the
/// end: tabs show no counts, and the archive opens even when it is empty.
class TaskFilters extends StatelessWidget {
  const TaskFilters({
    super.key,
    required this.selected,
    required this.onSelected,
    required this.onArchive,
  });

  final String selected;
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
                  (value: filter, label: context.tr(filter), count: null),
              ],
              selected: selected,
              onSelected: onSelected,
            ),
          ),
        ),
        IconButton(
          key: const ValueKey('task-archive'),
          tooltip: context.tr('archiveTab'),
          onPressed: onArchive,
          icon: AppIcon('archive', size: 20, color: colors.onSurfaceVariant),
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
