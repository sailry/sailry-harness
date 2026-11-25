import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../shell/assembly.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../resources/scope_picker.dart';
import 'configuration_fields.dart';
import 'team.dart';

class DraftSettings extends StatelessWidget {
  const DraftSettings({
    super.key,
    required this.host,
    required this.projects,
    required this.worktrees,
    required this.project,
    required this.worktree,
    required this.providers,
    required this.config,
    required this.team,
    required this.onProject,
    required this.onWorktree,
    required this.onConfig,
    this.locationEnabled = true,
  });

  final HostConnection host;
  final List<Map<String, dynamic>> projects;
  final List<Map<String, dynamic>> worktrees;
  final Map<String, dynamic>? project;
  final Map<String, dynamic>? worktree;
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final TeamChoices team;
  final ValueChanged<Map<String, dynamic>?> onProject;
  final ValueChanged<Map<String, dynamic>> onWorktree;
  final ValueChanged<Map<String, dynamic>> onConfig;
  final bool locationEnabled;

  String _projectLabel(BuildContext context, Map<String, dynamic>? value) =>
      value == null
      ? context.tr('unassignedChats')
      : text(value['name'], context.tr('project'));

  Widget _projectIcon(Map<String, dynamic>? value, {double size = 20}) =>
      value == null
      ? AppIcon('chat', size: size)
      : ProjectIcon(project: value, size: size);

  Future<void> _chooseProject(BuildContext context) async {
    final choice = await showAppSheet<Map<String, dynamic>>(
      context,
      context.tr('project'),
      child: ScopePicker(
        rows: [
          ScopeRow(
            leading: _projectIcon(null, size: scopeIconSize),
            title: _projectLabel(context, null),
            selected: project == null,
            onTap: () => Navigator.pop(context, <String, dynamic>{}),
          ),
          for (final item in projects)
            ScopeRow(
              leading: _projectIcon(item, size: scopeIconSize),
              title: _projectLabel(context, item),
              titleLines: 2,
              selected: item['id'] == project?['id'],
              onTap: () => Navigator.pop(context, item),
            ),
        ],
      ),
    );
    if (!context.mounted || choice == null) return;
    onProject(choice.isEmpty ? null : choice);
  }

  @override
  Widget build(BuildContext context) {
    final workspace = MobileAssemblyScope.maybeOf(context)?.draftWorkspace;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ConfigurationFields(
          host: host,
          providers: providers,
          config: config,
          onChanged: onConfig,
          team: team,
        ),
        const Divider(height: 16),
        ListTile(
          key: const ValueKey('draft-project'),
          leading: _projectIcon(project),
          title: Text(_projectLabel(context, project)),
          trailing: const AppIcon('chevron', size: 16),
          onTap: locationEnabled ? () => _chooseProject(context) : null,
        ),
        if (project != null && workspace != null)
          workspace(
            DraftWorkspaceContext(
              host: host,
              worktrees: worktrees,
              selected: worktree,
              enabled: locationEnabled,
              onChanged: onWorktree,
            ),
          ),
      ],
    );
  }
}
