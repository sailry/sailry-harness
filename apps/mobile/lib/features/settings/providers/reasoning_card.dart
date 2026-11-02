import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import 'parts.dart';
import 'rules.dart';

/// Ordered reasoning efforts with their raw values, a default choice and
/// free-form additions, like Desktop's effort rows.
class ReasoningCard extends StatefulWidget {
  const ReasoningCard({super.key, required this.model, required this.onSave});
  final Json model;

  /// Saves an edited model; null while another edit is pending.
  final Future<bool> Function(Json model)? onSave;

  @override
  State<ReasoningCard> createState() => _ReasoningCardState();
}

class _ReasoningCardState extends State<ReasoningCard> {
  final _input = TextEditingController();

  @override
  void dispose() {
    _input.dispose();
    super.dispose();
  }

  Future<void> _add() async {
    final edited = addEffort(widget.model, _input.text);
    if (!accept(context, edited == null ? 'providerEffortInvalid' : null)) {
      return;
    }
    // Keep the draft when the save fails.
    if (await widget.onSave!(edited!) && mounted) _input.clear();
  }

  @override
  Widget build(BuildContext context) {
    final efforts = [...?widget.model['efforts'] as List?];
    final fallback = effortKey(widget.model['default_effort']);
    final save = widget.onSave;
    return SectionCard(
      title: context.tr('providerEfforts'),
      count: efforts.length,
      children: [
        for (final (index, effort) in efforts.indexed)
          _Row(
            value: effortValue(effort),
            selected: effortKey(effort) == fallback,
            onDefault: save == null
                ? null
                : () => save(setDefaultEffort(widget.model, effort as Object)),
            onRemove: save == null
                ? null
                : () => save(removeEffort(widget.model, index)),
          ),
        Padding(
          padding: const EdgeInsets.symmetric(vertical: 10),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('effort-input'),
                  controller: _input,
                  enabled: save != null,
                  autocorrect: false,
                  enableSuggestions: false,
                  textInputAction: TextInputAction.done,
                  onSubmitted: (_) => _add(),
                  decoration: InputDecoration(
                    isDense: true,
                    hintText: context.tr('providerEffortPlaceholder'),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              TextButton.icon(
                key: const ValueKey('effort-add'),
                onPressed: save == null || efforts.length >= maxEfforts
                    ? null
                    : _add,
                icon: const AppIcon('plus', size: 16),
                label: Text(context.tr('add')),
              ),
            ],
          ),
        ),
      ],
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({
    required this.value,
    required this.selected,
    required this.onDefault,
    required this.onRemove,
  });
  final String value;
  final bool selected;
  final VoidCallback? onDefault;
  final VoidCallback? onRemove;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final label = context.tr(
      selected ? 'providerEffortDefault' : 'providerEffortSetDefault',
    );
    return ConstrainedBox(
      constraints: const BoxConstraints(minHeight: 48),
      child: Row(
        children: [
          Expanded(
            child: Text(
              value,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 15, fontFamily: 'monospace'),
            ),
          ),
          IconButton(
            tooltip: label,
            isSelected: selected,
            onPressed: selected ? null : onDefault,
            icon: AppIcon(
              'check',
              size: 18,
              color: selected
                  ? SailryTheme.accentColor(context, AccentTone.green)
                  : colors.onSurfaceVariant.withValues(alpha: .4),
            ),
          ),
          IconButton(
            tooltip: context.tr('delete'),
            onPressed: onRemove,
            icon: AppIcon('close', size: 18, color: colors.onSurfaceVariant),
          ),
        ],
      ),
    );
  }
}
