import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../runtime/json.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import '../conversations/live/project_bar.dart';
import 'contributions.dart';
import 'overview_cards.dart';
import 'project_form.dart';
import 'scope_picker.dart';
import 'workspace.dart';
import '../../ui/theme.dart';

/// Opens a project's resources without changing the chat list's filter.
void openProjectResources(
  BuildContext context,
  HostConnection host,
  String project,
) {
  final trees = projectWorktrees(host, project: project);
  final tree =
      trees.where((tree) => tree['main'] == true).firstOrNull ??
      trees.firstOrNull;
  if (tree == null) {
    showToast(context, context.tr('resourceNoWorkspace'));
    return;
  }
  pushPage(
    context,
    ResourcesPage(
      hostId: host.id,
      worktreeId: text(tree['id']),
      projectId: project,
    ),
  );
}

class ResourcesPage extends StatefulWidget {
  const ResourcesPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.projectId,
  });
  final String? hostId;
  final String? worktreeId;
  final String? projectId;
  @override
  State<ResourcesPage> createState() => _ResourcesPageState();
}

class _ResourcesPageState extends State<ResourcesPage> {
  bool get _fixedScope => widget.hostId != null || widget.worktreeId != null;
  bool get _workspaceSelectable => !_fixedScope || widget.projectId != null;
  String? _selectedProjectTree;
  int _revision = 0;

  Future<void> _host() async {
    final session = AppSession.of(context);
    final current = session.selectedHost?.id;
    final id = await showHostPicker(context, session, selected: current);
    if (id != null && mounted) {
      session.selectHost(id);
    }
  }

  Future<void> _workspace(HostConnection host) async {
    if (widget.projectId case final project?) {
      final id = await showLiveWorkspacePicker(
        context,
        host,
        _selectedProjectTree ?? widget.worktreeId,
        project: project,
      );
      if (mounted && id != null) {
        setState(() => _selectedProjectTree = id);
      }
      return;
    }
    final session = AppSession.of(context);
    final id = await showLiveWorkspacePicker(
      context,
      host,
      session.selectedWorktreeId,
      actions: [
        if (host.connected)
          Builder(
            builder: (context) => ScopeRow(
              leading: const AppIcon('plus', size: scopeIconSize),
              title: context.tr('hostRegisterProject'),
              onTap: () {
                Navigator.pop(context);
                showAppSheet(
                  this.context,
                  this.context.tr('hostRegisterProject'),
                  child: ProjectForm(host: host),
                );
              },
            ),
          ),
        if (session.hosts.length > 1)
          Builder(
            builder: (context) => ScopeRow(
              leading: const AppIcon('server', size: scopeIconSize),
              title: context.tr('selectHost'),
              subtitle: host.label,
              trailing: const AppIcon('chevron'),
              onTap: () {
                Navigator.pop(context);
                _host();
              },
            ),
          ),
      ],
    );
    if (mounted && id != null) session.selectWorkspace(host.id, id);
  }

  Future<void> _open(Widget page) async {
    await pushPage(context, page);
    if (mounted) setState(() => _revision++);
  }

  @override
  Widget build(BuildContext context) {
    final session = AppSession.of(context);
    final host = widget.hostId == null
        ? session.selectedHost
        : session.host(widget.hostId!);
    final target = resourceTarget(
      context,
      widget.hostId,
      _selectedProjectTree ?? widget.worktreeId,
    );
    final features = ProjectResourcesScope.of(context);
    final view = host != null && host.connected && target != null
        ? (
            host: host,
            worktree: text(target.worktree['id']),
            open: _open,
            revision: _revision,
          )
        : null;
    return PageFrame(
      title: context.tr('resources'),
      // A project's resources belong to one host; name it under the title.
      subtitle: host == null
          ? null
          : Text(
              host.label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: TypeScale.caption,
                color: Theme.of(
                  context,
                ).colorScheme.onSurfaceVariant.withValues(alpha: .65),
              ),
            ),
      onRefresh: host == null
          ? null
          : () async {
              await Future.wait([session.refreshHosts(), host.refresh()]);
            },
      failure: host?.connected != true
          ? HostState(added: host != null, failure: host?.error)
          : null,
      empty: host?.connected == true && target == null
          ? EmptyState(message: context.tr('resourceNoWorkspace'))
          : null,
      actions: [
        if (view != null)
          for (final feature in features)
            if (feature.action case final build?) build(context, view),
        // Keep host choice available while no workspace is usable.
        if (view == null && !_fixedScope && session.hosts.isNotEmpty)
          RoundButton(
            icon: 'server',
            tooltip: context.tr('selectHost'),
            onPressed: _host,
          ),
      ],
      child: view != null && target != null
          ? _overview(view, target, features)
          : host != null && host.connected && _workspaceSelectable
          ? ProjectBar(
              project: null,
              title: context.tr('selectWorkspace'),
              detail: host.label,
              onTap: () => _workspace(host),
            )
          : const SizedBox.shrink(),
    );
  }

  Widget _overview(
    ProjectResourceView view,
    ResourceTarget target,
    List<ProjectResourceFeature> features,
  ) {
    final host = view.host;
    final project = objects(host.snapshot['projects'])
        .where((project) => project['id'] == target.worktree['project'])
        .firstOrNull;
    final name = project != null && target.worktree['main'] == true
        ? text(project['name'])
        : worktreeLabel(target.worktree);
    Widget heading(String? detail) => ProjectBar(
      project: {...?project, 'name': name},
      detail: detail,
      onTap: _workspaceSelectable ? () => _workspace(host) : null,
    );
    final header = features
        .map((feature) => feature.header)
        .whereType<ProjectResourceHeader>()
        .firstOrNull;
    final entries = features
        .map((feature) => feature.entry)
        .whereType<ProjectResourceEntry>()
        .toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        header == null ? heading(null) : header(view, heading),
        if (entries.isNotEmpty) ...[
          const SizedBox(height: 12),
          ListenableBuilder(
            listenable: Listenable.merge([
              for (final feature in features)
                if (feature.entryChanges case final listen?) listen(view),
            ]),
            builder: (context, _) => ResourceEntries(
              entries: [for (final build in entries) build(context, view)],
            ),
          ),
        ],
      ],
    );
  }
}
