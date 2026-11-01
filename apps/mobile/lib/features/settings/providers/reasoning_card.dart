import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/theme.dart';
import 'parts.dart';
import 'rules.dart';

/// Available reasoning efforts and the default one, limited to the API's
/// choices like Desktop.
class ReasoningCard extends StatelessWidget {
  const ReasoningCard({
    super.key,
    required this.api,
    required this.model,
    required this.onChanged,
  });
  final String api;
  final Json model;
  final ValueChanged<Json>? onChanged;

  @override
  Widget build(BuildContext context) {
    final options = effortOptions(api, model);
    if (options.isEmpty) return const SizedBox.shrink();
    final efforts = [...?model['efforts'] as List?];
    final selected = efforts.map(effortKey).toSet();
    final fallback = effortKey(model['default_effort']);
    return SectionCard(
      title: context.tr('providerEfforts'),
      children: [
        Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                context.tr('providerEffortChoices'),
                style: dimText(context, 12),
              ),
              const SizedBox(height: 8),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  for (final effort in options)
                    _Chip(
                      label: effortLabel(context, effort),
                      selected: selected.contains(effortKey(effort)),
                      onTap: onChanged == null
                          ? null
                          : () => onChanged!(
                              toggleEffort(model, effort, options),
                            ),
                    ),
                ],
              ),
              if (efforts.isNotEmpty) ...[
                const SizedBox(height: 14),
                Text(
                  context.tr('providerDefault'),
                  style: dimText(context, 12),
                ),
                const SizedBox(height: 8),
                _Segments(
                  labels: [
                    for (final effort in efforts) effortLabel(context, effort),
                  ],
                  selected: efforts.indexWhere(
                    (effort) => effortKey(effort) == fallback,
                  ),
                  onSelected: onChanged == null
                      ? null
                      : (index) => onChanged!(
                          setDefaultEffort(model, efforts[index] as Object),
                        ),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }
}

String effortLabel(BuildContext context, Object? effort) {
  final budget = effort is Map ? effort['budget'] : null;
  if (budget is num) {
    return budget < 0 ? context.tr('effortDynamic') : '${budget.toInt()}';
  }
  return switch (effort) {
    'none' => context.tr('effortNone'),
    'minimal' => context.tr('effortMinimal'),
    'low' => context.tr('effortLow'),
    'medium' => context.tr('effortMedium'),
    'high' => context.tr('effortHigh'),
    'xhigh' => context.tr('effortXhigh'),
    'max' => context.tr('effortMax'),
    _ => effortKey(effort),
  };
}

class _Chip extends StatelessWidget {
  const _Chip({required this.label, required this.selected, this.onTap});
  final String label;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final accent = SailryTheme.accentColor(context, AccentTone.blue);
    return Semantics(
      selected: selected,
      button: true,
      child: Material(
        color: selected
            ? accent.withValues(alpha: .18)
            : SailryTheme.insetFill(context),
        shape: const StadiumBorder(),
        child: InkWell(
          customBorder: const StadiumBorder(),
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 13, vertical: 6),
            child: Text(
              label,
              style: TextStyle(
                fontSize: 13,
                color: selected ? accent : colors.onSurfaceVariant,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _Segments extends StatelessWidget {
  const _Segments({
    required this.labels,
    required this.selected,
    required this.onSelected,
  });
  final List<String> labels;
  final int selected;
  final ValueChanged<int>? onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: SailryTheme.insetFill(context),
        borderRadius: BorderRadius.circular(12),
      ),
      child: Padding(
        padding: const EdgeInsets.all(3),
        child: Row(
          children: [
            for (final (index, label) in labels.indexed)
              Expanded(
                child: Semantics(
                  selected: index == selected,
                  button: true,
                  child: Material(
                    color: index == selected
                        ? colors.surfaceContainerHigh
                        : Colors.transparent,
                    borderRadius: BorderRadius.circular(9),
                    child: InkWell(
                      borderRadius: BorderRadius.circular(9),
                      onTap: onSelected == null || index == selected
                          ? null
                          : () => onSelected!(index),
                      child: Padding(
                        padding: const EdgeInsets.symmetric(vertical: 7),
                        child: Text(
                          label,
                          textAlign: TextAlign.center,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 13,
                            fontWeight: index == selected
                                ? FontWeight.w500
                                : null,
                            color: index == selected
                                ? colors.onSurface
                                : colors.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
