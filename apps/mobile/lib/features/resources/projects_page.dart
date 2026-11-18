import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../runtime/catalog.dart';
import '../../runtime/json.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/project_icon.dart';
import '../../ui/swipe_action.dart';
import 'host/host_sheet.dart';
import 'project_form.dart';
import 'resources_page.dart';

/// The selected host's projects: a row opens resources and a left swipe edits
/// or removes the project. An embedded view can supply its own host.
class ProjectsPage extends StatelessWidget {
  const ProjectsPage({super.key, this.host});
  final HostConnection? host;

  Future<void> _selectHost(BuildContext context, AppSession session) async {
    final current = session.selectedHost?.id;
    final id = await showAppSheet<String>(
      context,
      context.tr('selectHost'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final host in session.hosts)
              HostRow(
                host: host,
                selected: host.id == current,
                onTap: () => Navigator.pop(context, host.id),
              ),
          ],
        ),
      ),
    );
    if (id != null && context.mounted) session.selectHost(id);
  }

  void _edit(
    BuildContext context,
    HostConnection source,
    Map<String, dynamic> project,
  ) => showAppSheet(
    context,
    context.tr('editProject'),
    child: ProjectForm(host: source, project: project),
  );

  Future<void> _remove(
    BuildContext context,
    HostConnection source,
    Map<String, dynamic> project,
  ) => showAppSheet(
    context,
    context.tr('removeProject'),
    child: ProjectForm.remove(host: source, project: project),
  );

  @override
  Widget build(BuildContext context) {
    final session = AppSession.maybeOf(context);
    final listenable = session ?? host;
    if (listenable == null) return const SizedBox.shrink();
    return ListenableBuilder(
      listenable: listenable,
      builder: (context, _) {
        final current = session == null ? host : session.selectedHost;
        final hosts = [?current];
        final projects = hostRecords(hosts, 'projects');
        final connected = hosts.any((source) => source.connected);
        final colors = Theme.of(context).colorScheme;
        return PageFrame(
          title: context.tr('projects'),
          onRefresh: () async {
            await Future.wait([for (final source in hosts) source.refresh()]);
          },
          failure: !connected && projects.isEmpty
              ? HostState(added: current != null, failure: current?.error)
              : null,
          actions: [
            if (host == null && session != null && session.hosts.length > 1)
              RoundButton(
                icon: 'server',
                tooltip: context.tr('selectHost'),
                onPressed: () => _selectHost(context, session),
              ),
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
          child: SlidableAutoCloseBehavior(
            child: Column(
              children: [
                for (final (source, project) in projects)
                  Slidable(
                    key: ValueKey((source.id, project['id'])),
                    enabled: source.connected,
                    endActionPane: ActionPane(
                      motion: const DrawerMotion(),
                      extentRatio: .46,
                      children: [
                        SwipeAction(
                          onPressed: (_) => _edit(context, source, project),
                          color: colors.secondaryContainer,
                          foreground: colors.onSecondaryContainer,
                          icon: Icons.edit_outlined,
                          label: context.tr('edit'),
                        ),
                        SwipeAction(
                          onPressed: (_) => _remove(context, source, project),
                          color: colors.error,
                          foreground: colors.onError,
                          icon: Icons.delete_outline,
                          label: context.tr('delete'),
                        ),
                      ],
                    ),
                    child: ListTile(
                      // Tall enough for the swipe actions' icon and label.
                      minTileHeight: 72,
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
                          : () => openProjectResources(
                              context,
                              source,
                              text(project['id']),
                            ),
                    ),
                  ),
              ],
            ),
          ),
        );
      },
    );
  }
}
