import 'dart:async';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/toast.dart';
import 'configuration_fields.dart';
import 'draft_branches.dart';
import 'presentation.dart' show failureLabel;
import 'team.dart';

class DraftSettings extends StatefulWidget {
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

  @override
  State<DraftSettings> createState() => _DraftSettingsState();
}

class _DraftSettingsState extends State<DraftSettings> {
  late final _branches = DraftBranches(widget.host);

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  @override
  void didUpdateWidget(DraftSettings oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.project?['id'] != widget.project?['id'] ||
        oldWidget.worktree == null && widget.worktree != null) {
      unawaited(_load());
    }
  }

  Future<void> _load() async {
    await _branches.load(widget.worktree);
    final error = _branches.error;
    if (mounted && error != null) {
      showToast(context, failureLabel(error, translate: context.tr));
    }
  }

  @override
  void dispose() {
    _branches.dispose();
    super.dispose();
  }

  String _projectLabel(Map<String, dynamic>? project) => project == null
      ? context.tr('unassignedChats')
      : text(project['name'], context.tr('project'));

  Widget _projectIcon(Map<String, dynamic>? project) => project == null
      ? const AppIcon('chat', size: 20)
      : ProjectIcon(project: project, size: 20);

  Future<void> _choose({required bool chooseProject}) async {
    if (!chooseProject) unawaited(_load());
    final items = chooseProject ? widget.projects : widget.worktrees;
    final selected = chooseProject ? widget.project : widget.worktree;
    final choice = await showAppSheet<Map<String, dynamic>>(
      context,
      context.tr(chooseProject ? 'project' : 'gitBranches'),
      child: ListenableBuilder(
        listenable: _branches,
        builder: (context, _) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (chooseProject)
              ListTile(
                leading: _projectIcon(null),
                title: Text(_projectLabel(null)),
                trailing: selected == null
                    ? const AppIcon('check', size: 18)
                    : null,
                onTap: () => Navigator.pop(context, <String, dynamic>{}),
              ),
            for (final item in items)
              ListTile(
                leading: chooseProject
                    ? _projectIcon(item)
                    : const AppIcon('branch', size: 20),
                title: Text(
                  chooseProject
                      ? _projectLabel(item)
                      : _branches.label(item, context.tr('worktree')),
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
      ),
    );
    if (!mounted || choice == null) return;
    if (chooseProject) {
      widget.onProject(choice.isEmpty ? null : choice);
    } else {
      widget.onWorktree(choice);
    }
  }

  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      ConfigurationFields(
        host: widget.host,
        providers: widget.providers,
        config: widget.config,
        onChanged: widget.onConfig,
        team: widget.team,
      ),
      const Divider(height: 16),
      ListTile(
        key: const ValueKey('draft-project'),
        leading: _projectIcon(widget.project),
        title: Text(_projectLabel(widget.project)),
        trailing: const AppIcon('chevron', size: 16),
        onTap: widget.locationEnabled
            ? () => _choose(chooseProject: true)
            : null,
      ),
      if (widget.project != null)
        ListenableBuilder(
          listenable: _branches,
          builder: (context, _) => ListTile(
            key: const ValueKey('draft-worktree'),
            leading: const AppIcon('branch', size: 20),
            title: Text(
              _branches.label(widget.worktree, context.tr('worktree')),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            trailing: const AppIcon('chevron', size: 16),
            onTap: widget.locationEnabled && widget.worktrees.isNotEmpty
                ? () => _choose(chooseProject: false)
                : null,
          ),
        ),
    ],
  );
}
