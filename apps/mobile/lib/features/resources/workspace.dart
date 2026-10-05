import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
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
}) => showAppSheet<String>(
  context,
  tr('selectWorkspace'),
  child: Builder(
    builder: (context) {
      final projects = objects(host.snapshot['projects']);
      final worktrees = objects(
        host.snapshot['worktrees'],
      ).where((tree) => project == null || tree['project'] == project).toList();
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (worktrees.isEmpty) EmptyState(message: tr('resourceNoWorkspace')),
          for (final worktree in worktrees)
            ListTile(
              leading: const AppIcon('branch'),
              title: Text(worktreeLabel(worktree)),
              subtitle: Text(
                projects
                        .where(
                          (project) => project['id'] == worktree['project'],
                        )
                        .map((project) => text(project['name']))
                        .firstOrNull ??
                    text(worktree['path']),
              ),
              trailing: text(worktree['id']) == selected
                  ? const AppIcon('check')
                  : null,
              onTap: () => Navigator.pop(context, text(worktree['id'])),
            ),
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
    tr(label),
    child: Builder(
      builder: (context) => FormBody(
        children: [
          TextFormField(
            initialValue: initial,
            onChanged: (text) => value = text,
            autofocus: true,
            minLines: multiline ? 3 : 1,
            maxLines: multiline ? 6 : 1,
            decoration: InputDecoration(labelText: tr(label)),
          ),
          FilledButton(
            onPressed: () {
              if (value.trim().isNotEmpty) {
                Navigator.pop(context, value.trim());
              }
            },
            child: Text(tr('confirm')),
          ),
        ],
      ),
    ),
  );
}

typedef WorkspaceSelection = ({String project, String branch});

Future<WorkspaceSelection?> showWorkspacePicker(
  BuildContext context, {
  required String host,
  required String project,
  required String branch,
}) {
  var selectedProject = project;
  var selectedBranch = branch;
  final projects = host == 'Build Server'
      ? ['sailry-api', 'sailry']
      : ['sailry-web', 'sailry'];
  return showAppSheet<WorkspaceSelection>(
    context,
    tr('selectWorkspace'),
    child: StatefulBuilder(
      builder: (context, update) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(tr('project'), style: Theme.of(context).textTheme.titleSmall),
          const SizedBox(height: 8),
          for (final name in projects)
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const AppIcon('folder'),
              title: Text(name),
              trailing: name == selectedProject ? const AppIcon('check') : null,
              onTap: () => update(() => selectedProject = name),
            ),
          const SizedBox(height: 16),
          Text(tr('worktree'), style: Theme.of(context).textTheme.titleSmall),
          const SizedBox(height: 8),
          for (final name in ['main', 'feature/sign-in'])
            ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const AppIcon('branch'),
              title: Text(name),
              trailing: name == selectedBranch ? const AppIcon('check') : null,
              onTap: () => update(() => selectedBranch = name),
            ),
          const SizedBox(height: 16),
          FilledButton(
            onPressed: () => Navigator.pop(context, (
              project: selectedProject,
              branch: selectedBranch,
            )),
            child: Text(tr('select')),
          ),
        ],
      ),
    ),
  );
}

void showResourceNotice(BuildContext context, String key) {
  showToast(context, tr(key));
}
