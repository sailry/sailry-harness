import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../runtime/session.dart';
import '../../runtime/json.dart';

typedef ResourceTarget = ({HostConnection host, Map<String, dynamic> worktree});

ResourceTarget? resourceTarget(
  BuildContext context,
  String? hostId,
  String? worktreeId,
) {
  final session = AppSession.of(context);
  final host = hostId == null ? session.selectedHost : session.host(hostId);
  if (host == null) return null;
  final selected =
      worktreeId ??
      (host.id == session.selectedHost?.id ? session.selectedWorktreeId : null);
  if (selected == null) return null;
  for (final worktree in objects(host.snapshot['worktrees'])) {
    if (text(worktree['id']) == selected) {
      return (host: host, worktree: worktree);
    }
  }
  return null;
}

// Session scratch workspaces remain reachable through an explicit session
// target; a general resource picker lists registered projects only.
List<Map<String, dynamic>> projectWorktrees(
  HostConnection host, {
  String? project,
}) {
  final projects = objects(
    host.snapshot['projects'],
  ).map((value) => value['id']).whereType<String>().toSet();
  return objects(host.snapshot['worktrees'])
      .where(
        (tree) =>
            projects.contains(tree['project']) &&
            inProject(project, tree['project']),
      )
      .toList();
}

/// Project filter for conversations and workspaces outside any project.
/// Project IDs are never empty, so the empty string cannot collide.
const unassignedProject = '';

/// Whether a record's project falls inside a filter; null selects everything.
bool inProject(String? filter, Object? project) => filter == null
    ? true
    : filter == unassignedProject
    ? project == null
    : project == filter;

String worktreeLabel(Map<String, dynamic> worktree) =>
    text(worktree['path'])
        .replaceAll('\\', '/')
        .split('/')
        .where((part) => part.isNotEmpty)
        .lastOrNull ??
    '';

Future<String?> showLiveWorkspacePicker(
  BuildContext context,
  HostConnection host,
  String? selected, {
  String? project,
  List<Widget> actions = const [],
}) => showAppSheet<String>(
  context,
  context.tr('selectWorkspace'),
  actions: actions,
  child: ListenableBuilder(
    listenable: host,
    builder: (context, _) {
      final projects = objects(
        host.snapshot['projects'],
      ).where((item) => project == null || item['id'] == project).toList();
      final worktrees = projectWorktrees(host, project: project);
      final groups = <(Map<String, dynamic>, List<Map<String, dynamic>>)>[
        for (final item in projects)
          (
            item,
            worktrees.where((tree) => tree['project'] == item['id']).toList(),
          ),
      ];
      final colors = Theme.of(context).colorScheme;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (projects.isEmpty)
            EmptyState(message: context.tr('resourceNoWorkspace')),
          for (final (item, trees) in groups) ...[
            if (trees.isEmpty)
              ListTile(
                leading: const AppIcon('folder'),
                title: Text(text(item['name'])),
                subtitle: Text(
                  text(item['path']),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                enabled: false,
              ),
            if (groups.length > 1 && trees.isNotEmpty)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                child: Text(
                  text(item['name']),
                  style: TextStyle(
                    fontSize: 12,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ),
            for (final worktree in trees)
              ListTile(
                leading: AppIcon(
                  worktree['main'] == true ? 'folder' : 'branch',
                ),
                title: Text(
                  worktree['main'] == true
                      ? text(item['name'])
                      : worktreeLabel(worktree),
                ),
                subtitle: Text(
                  text(worktree['path']),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                selected: text(worktree['id']) == selected,
                enabled: host.connected,
                onTap: host.connected
                    ? () => Navigator.pop(context, text(worktree['id']))
                    : null,
              ),
          ],
        ],
      );
    },
  ),
);

Future<String?> askResourceText(
  BuildContext context,
  String label, {
  bool multiline = false,
  String initial = '',
}) async {
  var value = initial;
  return showAppSheet<String>(
    context,
    context.tr(label),
    child: Builder(
      builder: (context) => FormBody(
        children: [
          TextFormField(
            initialValue: initial,
            onChanged: (text) => value = text,
            autofocus: true,
            minLines: multiline ? 3 : 1,
            maxLines: multiline ? 6 : 1,
            decoration: InputDecoration(labelText: context.tr(label)),
          ),
          FilledButton(
            onPressed: () {
              if (value.trim().isNotEmpty) {
                Navigator.pop(context, value.trim());
              }
            },
            child: Text(context.tr('confirm')),
          ),
        ],
      ),
    ),
  );
}
