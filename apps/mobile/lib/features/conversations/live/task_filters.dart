import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';

/// Status filters shown as tabs; archive and terminal views are reached elsewhere.
const taskFilters = ['all', 'waiting', 'running', 'completed'];

StatusTone? _filterTone(String filter) => switch (filter) {
  'waiting' => StatusTone.warning,
  'running' => StatusTone.running,
  'completed' => StatusTone.success,
  _ => null,
};

/// Equal-width glass tabs with each status's count and list dot color.
class TaskFilters extends StatelessWidget {
  const TaskFilters({
    super.key,
    required this.selected,
    required this.counts,
    required this.onSelected,
  });

  final String selected;
  final Map<String, int> counts;
  final ValueChanged<String> onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Surface(
      radius: 16,
      padding: const EdgeInsets.all(4),
      child: Row(
        spacing: 4,
        children: [
          for (final filter in taskFilters)
            Expanded(child: _tab(context, colors, filter)),
        ],
      ),
    );
  }

  Widget _tab(BuildContext context, ColorScheme colors, String filter) {
    final active = filter == selected;
    final count = counts[filter] ?? 0;
    final tone = _filterTone(filter);
    return Semantics(
      selected: active,
      child: TextButton(
        key: ValueKey('task-filter-$filter'),
        style: TextButton.styleFrom(
          padding: const EdgeInsets.symmetric(horizontal: 4),
          minimumSize: const Size(0, 34),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          backgroundColor: active
              ? colors.onSurface.withValues(alpha: .08)
              : Colors.transparent,
          foregroundColor: active ? colors.onSurface : colors.onSurfaceVariant,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
          ),
        ),
        onPressed: () => onSelected(filter),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (tone != null && count > 0) ...[
              Container(
                width: 6,
                height: 6,
                decoration: BoxDecoration(
                  color: SailryTheme.statusColor(context, tone),
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: 5),
            ],
            Flexible(
              child: Text(
                context.tr(filter),
                maxLines: 1,
                overflow: TextOverflow.fade,
                softWrap: false,
                style: TextStyle(
                  fontSize: 14,
                  fontWeight: active ? FontWeight.w500 : FontWeight.w400,
                ),
              ),
            ),
            if (tone != null && count > 0) ...[
              const SizedBox(width: 4),
              Text(
                '$count',
                style: TextStyle(
                  fontSize: 12,
                  color: colors.onSurfaceVariant.withValues(alpha: .7),
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ],
          ],
        ),
      ),
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
