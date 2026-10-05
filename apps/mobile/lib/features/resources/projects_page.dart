import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/project_icon.dart';
import 'project_form.dart';
import 'resources_page.dart';

class ProjectsPage extends StatelessWidget {
  const ProjectsPage({super.key, required this.host});
  final HostConnection host;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: host,
    builder: (context, _) {
      final projects = objects(host.snapshot['projects']);
      return PageFrame(
        title: tr('project'),
        failure: !host.connected ? const HostState(added: true) : null,
        actions: [
          RoundButton(
            icon: 'plus',
            tooltip: tr('hostRegisterProject'),
            onPressed: !host.connected
                ? null
                : () => showAppSheet(
                    context,
                    tr('hostRegisterProject'),
                    child: ProjectForm(host: host),
                  ),
          ),
        ],
        empty: projects.isEmpty
            ? EmptyState(message: tr('conversationNoProject'))
            : null,
        child: Column(
          children: [
            for (final project in projects)
              ListTile(
                leading: ProjectIcon(project: project),
                title: Text(text(project['name'])),
                subtitle: Text(
                  text(project['path']),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                trailing: const AppIcon('chevron'),
                onTap: () {
                  final tree = objects(host.snapshot['worktrees'])
                      .where((tree) => tree['project'] == project['id'])
                      .firstOrNull;
                  if (tree != null) {
                    pushPage(
                      context,
                      ResourcesPage(
                        hostId: host.id,
                        worktreeId: text(tree['id']),
                      ),
                    );
                  }
                },
              ),
          ],
        ),
      );
    },
  );
}
