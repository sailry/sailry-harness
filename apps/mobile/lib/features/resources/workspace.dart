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
  final worktrees = objects(host.snapshot['worktrees']);
  for (final worktree in worktrees) {
    if (worktreeId == null || text(worktree['id']) == worktreeId) {
      return (host: host, worktree: worktree);
    }
  }
  return null;
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
  child: Builder(
    builder: (context) {
      final projects = objects(host.snapshot['projects']);
      final worktrees = objects(
        host.snapshot['worktrees'],
      ).where((tree) => inProject(project, tree['project'])).toList();
      // Grouped like the Desktop sidebar: each project, then chats.
      final groups = <(String, List<Map<String, dynamic>>)>[
        for (final item in projects)
          (
            text(item['name']),
            worktrees.where((tree) => tree['project'] == item['id']).toList(),
          ),
        (
          context.tr('unassignedChats'),
          worktrees.where((tree) => tree['project'] == null).toList(),
        ),
      ].where((group) => group.$2.isNotEmpty).toList();
      final colors = Theme.of(context).colorScheme;
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (worktrees.isEmpty)
            EmptyState(message: context.tr('resourceNoWorkspace')),
          for (final (label, trees) in groups) ...[
            if (groups.length > 1)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                child: Text(
                  label,
                  style: TextStyle(
                    fontSize: 12,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ),
            for (final worktree in trees)
              ListTile(
                leading: AppIcon(
                  worktree['project'] == null ? 'chat' : 'branch',
                ),
                title: Text(worktreeLabel(worktree)),
                subtitle: Text(
                  text(worktree['path']),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                trailing: text(worktree['id']) == selected
                    ? const AppIcon('check')
                    : null,
                onTap: () => Navigator.pop(context, text(worktree['id'])),
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
