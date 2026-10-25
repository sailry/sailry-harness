import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
import 'model_controls.dart';
import '../../../ui/form.dart';
import 'modes.dart';
import 'presentation.dart' show failureLabel;

class ConfigurationFields extends StatefulWidget {
  const ConfigurationFields({
    super.key,
    required this.host,
    required this.providers,
    required this.config,
    required this.onChanged,
    this.onCommitted,
    this.enabled = true,
  });

  final HostConnection host;
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;

  @override
  State<ConfigurationFields> createState() => _ConfigurationFieldsState();
}

class _ConfigurationFieldsState extends State<ConfigurationFields> {
  late ConversationModes _modes;
  Object? _reported;

  @override
  void initState() {
    super.initState();
    _observe();
  }

  void _observe() {
    _reported = null;
    _modes = ConversationModes(widget.host)..addListener(_changed);
  }

  void _changed() {
    final failure = _modes.error;
    if (failure != null && !identical(failure, _reported)) {
      _reported = failure;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && identical(failure, _modes.error)) {
          showToast(context, failureLabel(failure, translate: context.tr));
        }
      });
    }
    if (mounted) setState(() {});
  }

  @override
  void didUpdateWidget(ConfigurationFields oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.host != widget.host) {
      _modes.removeListener(_changed);
      _modes.dispose();
      _observe();
    }
  }

  @override
  void dispose() {
    _modes.removeListener(_changed);
    _modes.dispose();
    super.dispose();
  }

  void _commit(Map<String, dynamic> value) {
    widget.onChanged(value);
    widget.onCommitted?.call(value);
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final config = widget.config;
    final selectedMode = _modes.selected(config);
    final pluginModes = _modes.choices(config);
    Widget choices(
      String field,
      List<(String, String)> values,
    ) => SegmentedButton<String>(
      key: ValueKey('configuration-$field'),
      expandedInsets: EdgeInsets.zero,
      showSelectedIcon: false,
      emptySelectionAllowed: field == 'mode',
      segments: [
        for (final (value, label) in values)
          ButtonSegment(value: value, label: Text(context.tr(label))),
        if (field == 'mode')
          for (final mode in pluginModes)
            ButtonSegment(
              value: mode.key,
              label: Text(mode.label(Localizations.localeOf(context))),
              enabled: mode.enabled,
            ),
      ],
      selected: field == 'mode' ? {?selectedMode} : {config[field] as String},
      onSelectionChanged: widget.enabled && (field != 'mode' || _modes.ready)
          ? (selection) {
              if (selection.isEmpty) return;
              final next = field == 'mode'
                  ? _modes.select(config, selection.single)
                  : {...config, field: selection.single};
              if (next != null) _commit(next);
            }
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
          providers: widget.providers,
          config: config,
          enabled: widget.enabled,
          onChanged: widget.onChanged,
          onCommitted: widget.onCommitted,
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
