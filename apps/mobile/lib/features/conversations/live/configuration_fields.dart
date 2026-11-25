import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../shell/assembly.dart';
import 'model_controls.dart';
import '../../../ui/kit.dart';
import '../../../ui/list_group.dart';
import '../../../ui/theme.dart';
import 'team.dart';
import 'team_controls.dart';

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
  /// The team list replaces the fields until a team is chosen.
  bool _choosingTeam = false;

  void _commit(Map<String, dynamic> value) {
    widget.onChanged(value);
    widget.onCommitted?.call(value);
  }

  @override
  Widget build(BuildContext context) {
    final config = widget.config;
    final contribution = MobileAssemblyScope.maybeOf(
      context,
    )?.conversationConfiguration;
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
        if (contribution != null)
          contribution(
            ConversationConfigurationContext(
              host: widget.host,
              config: config,
              enabled: widget.enabled,
              onChanged: _commit,
            ),
          ),
        // Each level says what it allows; full access, which reaches past
        // the project, wears the warning color.
        ListGroup(
          key: const ValueKey('configuration-permission'),
          title: context.tr('conversationPermission'),
          children: [
            for (final (value, label) in const [
              ('ask', 'conversationAsk'),
              ('project', 'conversationProject'),
              ('full', 'conversationFull'),
            ])
              ListGroupRow(
                key: ValueKey('configuration-permission-$value'),
                title: context.tr(label),
                subtitle: context.tr('${label}Description'),
                color: value == 'full'
                    ? SailryTheme.statusColor(context, StatusTone.warning)
                    : null,
                selected: config['permission'] == value,
                onTap: widget.enabled
                    ? () => _commit({...config, 'permission': value})
                    : null,
              ),
          ],
        ),
      ],
    );
  }
}
