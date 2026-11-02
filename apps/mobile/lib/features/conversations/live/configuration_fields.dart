import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
import 'choice_row.dart';
import 'model_controls.dart';
import '../../../ui/kit.dart';
import 'modes.dart';
import 'team.dart';
import 'team_controls.dart';
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
    this.team,
  });

  final HostConnection host;
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;
  final TeamChoices? team;

  @override
  State<ConfigurationFields> createState() => _ConfigurationFieldsState();
}

class _ConfigurationFieldsState extends State<ConfigurationFields> {
  late ConversationModes _modes;
  Object? _reported;

  /// The team list replaces the fields until a team is chosen.
  bool _choosingTeam = false;

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
    final modes = [
      for (final (value, label) in const [
        ('code', 'conversationCode'),
        ('plan', 'conversationPlan'),
        ('flagship', 'conversationFlagship'),
      ])
        (value, context.tr(label), true),
      for (final mode in _modes.choices(config))
        (mode.key, mode.label(Localizations.localeOf(context)), mode.enabled),
    ];
    final current = modes.where((mode) => mode.$1 == selectedMode).firstOrNull;
    Widget choices(String field, List<(String, String)> values) =>
        SegmentedButton<String>(
          key: ValueKey('configuration-$field'),
          expandedInsets: EdgeInsets.zero,
          showSelectedIcon: false,
          segments: [
            for (final (value, label) in values)
              ButtonSegment(value: value, label: Text(context.tr(label))),
          ],
          selected: {config[field] as String},
          onSelectionChanged: widget.enabled
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
    // Plugin packages can add modes, so a list scales where segments would not.
    final mode = ListTile(
      key: const ValueKey('configuration-mode'),
      // Match the sheet's choice rows instead of the dense sheet tiles.
      contentPadding: const EdgeInsets.symmetric(horizontal: 10),
      minTileHeight: 44,
      minVerticalPadding: 0,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
      titleTextStyle: TextStyle(fontSize: 15, color: colors.onSurface),
      enabled: widget.enabled && _modes.ready,
      title: Text(context.tr('conversationMode')),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            current?.$2 ?? '',
            style: TextStyle(fontSize: 15, color: colors.onSurfaceVariant),
          ),
          const SizedBox(width: 6),
          AppIcon('chevron', size: 14, color: colors.onSurfaceVariant),
        ],
      ),
      onTap: () async {
        final key = await showAppSheet<String>(
          context,
          context.tr('conversationMode'),
          child: Builder(
            builder: (sheet) => Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final (value, label, enabled) in modes)
                  Opacity(
                    opacity: enabled ? 1 : .45,
                    child: ChoiceRow(
                      label: label,
                      selected: value == selectedMode,
                      onTap: enabled
                          ? () => Navigator.pop(sheet, value)
                          : () {},
                    ),
                  ),
              ],
            ),
          ),
        );
        if (key == null || key == selectedMode || !mounted) return;
        final next = _modes.select(widget.config, key);
        if (next != null) _commit(next);
      },
    );
    final team = widget.team;
    if (team != null && _choosingTeam) {
      return ListenableBuilder(
        listenable: team.listenable,
        builder: (context, _) => TeamList(
          state: team.read(),
          onBack: () => setState(() => _choosingTeam = false),
          onSelected: (selected) async {
            await team.onTeam(selected);
            if (mounted) setState(() => _choosingTeam = false);
          },
        ),
      );
    }
    return FormBody(
      children: [
        ModelControls(
          providers: widget.providers,
          config: config,
          enabled: widget.enabled,
          onChanged: widget.onChanged,
          onCommitted: widget.onCommitted,
          team: team,
          onTeam: widget.enabled
              ? () => setState(() => _choosingTeam = true)
              : null,
        ),
        mode,
        choices('permission', [
          ('ask', 'conversationAsk'),
          ('project', 'conversationProject'),
          ('full', 'conversationFull'),
        ]),
      ],
    );
  }
}
