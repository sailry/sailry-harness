import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import 'configuration_fields.dart';

class DraftSettings extends StatelessWidget {
  const DraftSettings({
    super.key,
    required this.projects,
    required this.worktrees,
    required this.project,
    required this.worktree,
    required this.providers,
    required this.config,
    required this.onProject,
    required this.onWorktree,
    required this.onConfig,
  });

  final List<Map<String, dynamic>> projects;
  final List<Map<String, dynamic>> worktrees;
  final Map<String, dynamic>? project;
  final Map<String, dynamic>? worktree;
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onProject;
  final ValueChanged<Map<String, dynamic>> onWorktree;
  final ValueChanged<Map<String, dynamic>> onConfig;

  Future<void> _choose(
    BuildContext context, {
    required bool chooseProject,
  }) async {
    final items = chooseProject ? projects : worktrees;
    final selected = chooseProject ? project : worktree;
    final choice = await showAppSheet<Map<String, dynamic>>(
      context,
      context.tr(chooseProject ? 'project' : 'worktree'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final item in items)
            ListTile(
              leading: chooseProject
                  ? ProjectIcon(project: item, size: 20)
                  : const AppIcon('branch', size: 20),
              title: Text(
                text(item[chooseProject ? 'name' : 'path']),
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
              trailing: item['id'] == selected?['id']
                  ? const AppIcon('check', size: 18)
                  : null,
              onTap: () => Navigator.pop(context, item),
            ),
        ],
      ),
    );
    if (context.mounted && choice != null) {
      (chooseProject ? onProject : onWorktree)(choice);
    }
  }

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      ListTile(
        key: const ValueKey('draft-project'),
        leading: ProjectIcon(project: project ?? {}, size: 20),
        title: Text(text(project?['name'], context.tr('project'))),
        trailing: const AppIcon('chevron', size: 16),
        onTap: projects.isEmpty
            ? null
            : () => _choose(context, chooseProject: true),
      ),
      ListTile(
        key: const ValueKey('draft-worktree'),
        leading: const AppIcon('branch', size: 20),
        title: Text(
          text(worktree?['path'], context.tr('worktree')),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        trailing: const AppIcon('chevron', size: 16),
        onTap: worktrees.isEmpty
            ? null
            : () => _choose(context, chooseProject: false),
      ),
      const Divider(height: 16),
      ConfigurationFields(
        providers: providers,
        config: config,
        onChanged: onConfig,
      ),
    ],
  );
}
