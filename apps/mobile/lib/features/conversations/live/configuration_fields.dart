import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import 'model_controls.dart';
import '../../../ui/form.dart';

class ConfigurationFields extends StatelessWidget {
  const ConfigurationFields({
    super.key,
    required this.providers,
    required this.config,
    required this.onChanged,
    this.onCommitted,
    this.enabled = true,
  });

  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;

  void _commit(Map<String, dynamic> value) {
    onChanged(value);
    onCommitted?.call(value);
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    Widget choices(String field, List<(String, String)> values) =>
        SegmentedButton<String>(
          key: ValueKey('configuration-$field'),
          expandedInsets: EdgeInsets.zero,
          showSelectedIcon: false,
          segments: [
            for (final (value, label) in values)
              ButtonSegment(value: value, label: Text(tr(label))),
          ],
          selected: {config[field] as String},
          onSelectionChanged: enabled
              ? (selection) => _commit({...config, field: selection.single})
              : null,
          style: SegmentedButton.styleFrom(
            side: BorderSide.none,
            backgroundColor: colors.surfaceContainer,
            selectedBackgroundColor: colors.surfaceContainerHigh,
            selectedForegroundColor: colors.onSurface,
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 10),
          ),
        );
    return FormBody(
      children: [
        ModelControls(
          providers: providers,
          config: config,
          enabled: enabled,
          onChanged: onChanged,
          onCommitted: onCommitted,
        ),
        choices('mode', [
          ('code', 'conversationCode'),
          ('plan', 'conversationPlan'),
        ]),
        choices('permission', [
          ('ask', 'conversationAsk'),
          ('project', 'conversationProject'),
          ('full', 'conversationFull'),
        ]),
      ],
    );
  }
}
