import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import 'parts.dart';
import 'rules.dart';

/// Ordered reasoning efforts with their raw values, a default choice and
/// free-form additions, like Desktop's effort rows, shown as one scrolling
/// line of chips. Tapping a chip makes it the default.
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
  bool _adding = false;

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
    // Keep the draft and the open field when the save fails.
    if (await widget.onSave!(edited!) && mounted) {
      _input.clear();
      setState(() => _adding = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final efforts = [...?widget.model['efforts'] as List?];
    final fallback = effortKey(widget.model['default_effort']);
    final save = widget.onSave;
    final full = efforts.length >= maxEfforts;
    return SectionCard(
      title: context.tr('providerEfforts'),
      count: efforts.length,
      children: [
        Padding(
          padding: const EdgeInsets.only(top: 4, bottom: 10),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 10,
            children: [
              if (efforts.isNotEmpty)
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    spacing: 8,
                    children: [
                      for (final (index, effort) in efforts.indexed)
                        _EffortChip(
                          value: effortValue(effort),
                          selected: effortKey(effort) == fallback,
                          onDefault: save == null
                              ? null
                              : () => save(
                                  setDefaultEffort(
                                    widget.model,
                                    effort as Object,
                                  ),
                                ),
                          onRemove: save == null
                              ? null
                              : () => save(removeEffort(widget.model, index)),
                        ),
                      if (!_adding)
                        _AddChip(
                          onPressed: save == null || full
                              ? null
                              : () => setState(() => _adding = true),
                        ),
                    ],
                  ),
                ),
              // An empty list shows the field directly.
              if (_adding || efforts.isEmpty)
                Row(
                  children: [
                    Expanded(
                      child: TextField(
                        key: const ValueKey('effort-input'),
                        controller: _input,
                        enabled: save != null,
                        autofocus: _adding,
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
                      onPressed: save == null || full ? null : _add,
                      icon: const AppIcon('plus', size: 16),
                      label: Text(context.tr('add')),
                    ),
                  ],
                ),
            ],
          ),
        ),
      ],
    );
  }
}

const _chipHeight = 34.0;

/// One effort. The default is filled with the brand selection like the
/// selected pill tab; the trailing cross removes the effort.
class _EffortChip extends StatelessWidget {
  const _EffortChip({
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
    final brand = SailryTheme.brand(context);
    final foreground = selected ? brand.color : colors.onSurfaceVariant;
    return Semantics(
      selected: selected,
      child: Material(
        key: ValueKey('effort-$value'),
        color: selected ? brand.selection : SailryTheme.insetFill(context),
        shape: StadiumBorder(
          side: BorderSide(
            color: selected ? Colors.transparent : colors.outlineVariant,
          ),
        ),
        clipBehavior: Clip.antiAlias,
        child: SizedBox(
          height: _chipHeight,
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Tooltip(
                message: context.tr(
                  selected
                      ? 'providerEffortDefault'
                      : 'providerEffortSetDefault',
                ),
                child: InkWell(
                  onTap: selected ? null : onDefault,
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 0, 4, 0),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (selected) ...[
                          AppIcon('check', size: 14, color: foreground),
                          const SizedBox(width: 4),
                        ],
                        Text(
                          value,
                          style: TextStyle(
                            fontSize: 13,
                            fontFamily: 'monospace',
                            color: foreground,
                            fontWeight: selected ? FontWeight.w500 : null,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              Tooltip(
                message: context.tr('delete'),
                child: InkWell(
                  onTap: onRemove,
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(4, 0, 10, 0),
                    child: AppIcon(
                      'close',
                      size: 12,
                      color: foreground.withValues(alpha: .7),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Trailing chip that reveals the effort field.
class _AddChip extends StatelessWidget {
  const _AddChip({required this.onPressed});
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Tooltip(
      message: context.tr('add'),
      child: Material(
        key: const ValueKey('effort-new'),
        color: Colors.transparent,
        shape: StadiumBorder(side: BorderSide(color: colors.outlineVariant)),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onPressed,
          child: SizedBox(
            width: _chipHeight + 6,
            height: _chipHeight,
            child: Center(
              child: AppIcon(
                'plus',
                size: 15,
                color: colors.onSurfaceVariant.withValues(
                  alpha: onPressed == null ? .4 : 1,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
