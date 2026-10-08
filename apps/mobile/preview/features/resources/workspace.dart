import 'package:sailry_mobile/ui/toast.dart';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';

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
    context.tr('selectWorkspace'),
    child: StatefulBuilder(
      builder: (context, update) => Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            context.tr('project'),
            style: Theme.of(context).textTheme.titleSmall,
          ),
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
          Text(
            context.tr('worktree'),
            style: Theme.of(context).textTheme.titleSmall,
          ),
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
            child: Text(context.tr('select')),
          ),
        ],
      ),
    ),
  );
}

void showResourceNotice(BuildContext context, String key) {
  showToast(context, context.tr(key));
}
