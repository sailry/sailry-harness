import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/catalog.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/project_icon.dart';
import 'project_form.dart';
import 'resources_page.dart';
import 'workspace.dart';

class ProjectsPage extends StatelessWidget {
  const ProjectsPage({super.key, required this.host});
  final HostConnection host;

  @override
  Widget build(BuildContext context) {
    final session = AppSession.maybeOf(context);
    return ListenableBuilder(
      listenable: session ?? host,
      builder: (context, _) {
        final current = session == null ? host : session.selectedHost;
        final hosts = [?current];
        final projects = hostRecords(hosts, 'projects');
        final connected = hosts.any((source) => source.connected);
        return PageFrame(
          title: context.tr('project'),
          onRefresh: () async {
            await Future.wait([for (final source in hosts) source.refresh()]);
          },
          failure: !connected && projects.isEmpty
              ? HostState(added: current != null, failure: current?.error)
              : null,
          actions: [
            if (current != null && current.connected)
              RoundButton(
                icon: 'plus',
                tooltip: context.tr('hostRegisterProject'),
                onPressed: () => showAppSheet(
                  context,
                  context.tr('hostRegisterProject'),
                  child: ProjectForm(host: current),
                ),
              ),
          ],
          empty: projects.isEmpty
              ? EmptyState(message: context.tr('conversationNoProject'))
              : null,
          child: Column(
            children: [
              for (final (source, project) in projects)
                ListTile(
                  key: ValueKey((source.id, project['id'])),
                  leading: ProjectIcon(project: project),
                  title: Text(text(project['name'])),
                  subtitle: Text(
                    text(project['path']),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: source.connected
                      ? const AppIcon('chevron')
                      : Text(context.tr('offline')),
                  enabled: source.connected,
                  onTap: !source.connected
                      ? null
                      : () {
                          final projectId = text(project['id']);
                          final trees = projectWorktrees(
                            source,
                            project: projectId,
                          );
                          final tree =
                              trees
                                  .where((tree) => tree['main'] == true)
                                  .firstOrNull ??
                              trees.firstOrNull;
                          if (tree != null) {
                            session?.selectWorkspace(
                              source.id,
                              text(tree['id']),
                            );
                            pushPage(
                              context,
                              ResourcesPage(
                                hostId: source.id,
                                worktreeId: text(tree['id']),
                              ),
                            );
                          } else {
                            session?.selectProject(source.id, projectId);
                          }
                        },
                ),
            ],
          ),
        );
      },
    );
  }
}
