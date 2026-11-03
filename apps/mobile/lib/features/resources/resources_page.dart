import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../runtime/json.dart';
import '../../ui/kit.dart';
import '../conversations/live/project_bar.dart';
import '../conversations/live/terminal_tasks.dart';
import '../terminal/terminal_page.dart';
import 'files_page.dart';
import 'git/summary.dart';
import 'git_page.dart';
import 'overview_cards.dart';
import 'ports_page.dart';
import 'workspace.dart';

class ResourcesPage extends StatefulWidget {
  const ResourcesPage({super.key, this.hostId, this.worktreeId});
  final String? hostId;
  final String? worktreeId;
  @override
  State<ResourcesPage> createState() => _ResourcesPageState();
}

class _ResourcesPageState extends State<ResourcesPage> {
  late String? _hostId = widget.hostId;
  late String? _worktreeId = widget.worktreeId;
  String? _scope;
  final _git = GitSummary();

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final id = _hostId ?? AppSession.of(context).selectedHost?.id;
    if (_scope != null && _scope != id) _worktreeId = null;
    _scope = id;
  }

  @override
  void dispose() {
    _git.dispose();
    super.dispose();
  }

  Future<void> _host() async {
    final session = AppSession.of(context);
    final id = await showAppSheet<String>(
      context,
      context.tr('selectHost'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final host in session.hosts)
              ListTile(
                title: Text(host.label),
                subtitle: host.connected
                    ? null
                    : Text(
                        context.tr(
                          versionMismatchKey(host.error) ?? 'hostDisconnected',
                        ),
                      ),
                onTap: () => Navigator.pop(context, host.id),
              ),
          ],
        ),
      ),
    );
    if (id != null && mounted) {
      session.selectHost(id);
      setState(() {
        if (widget.hostId != null) _hostId = id;
        _worktreeId = null;
      });
    }
  }

  Future<void> _workspace(HostConnection host) async {
    final session = AppSession.of(context);
    final id = await showLiveWorkspacePicker(
      context,
      host,
      _worktreeId,
      actions: [
        if (session.hosts.length > 1)
          Builder(
            builder: (context) => ListTile(
              leading: const AppIcon('server'),
              title: Text(context.tr('selectHost')),
              subtitle: Text(host.label),
              trailing: const AppIcon('chevron', size: 14),
              onTap: () {
                Navigator.pop(context);
                _host();
              },
            ),
          ),
      ],
    );
    if (mounted && id != null) setState(() => _worktreeId = id);
  }

  /// Returning from a resource page refreshes the Git summary it may change.
  Future<void> _open(Widget page) async {
    await pushPage(context, page);
    if (mounted) _git.load();
  }

  @override
  Widget build(BuildContext context) {
    final session = AppSession.of(context);
    final host = _hostId == null
        ? session.selectedHost
        : session.host(_hostId!);
    final target = resourceTarget(context, _hostId, _worktreeId);
    final ready = host != null && host.connected && target != null;
    final worktree = ready ? text(target.worktree['id']) : null;
    _git.track(ready ? host : null, worktree);
    final pushed = ModalRoute.canPopOf(context) == true;
    return PageFrame(
      title: context.tr('resources'),
      failure: host?.connected != true
          ? HostState(added: host != null, failure: host?.error)
          : null,
      empty: host?.connected == true && target == null
          ? EmptyState(message: context.tr('resourceNoWorkspace'))
          : null,
      actions: [
        if (ready)
          RoundButton(
            icon: 'terminal',
            tooltip: context.tr('terminal'),
            onPressed: () => pushPage(
              context,
              TerminalPage(hostId: host.id, worktreeId: worktree),
            ),
          ),
        // Without a workspace bar the host choice stays in the header.
        if (!ready && !pushed)
          RoundButton(
            icon: 'server',
            tooltip: context.tr('selectHost'),
            onPressed: session.hosts.isEmpty ? null : _host,
          ),
      ],
      child: ready
          ? ListenableBuilder(
              listenable: Listenable.merge([_git, host.ports]),
              builder: (context, _) => _overview(host, target),
            )
          : const SizedBox.shrink(),
    );
  }

  Widget _overview(HostConnection host, ResourceTarget target) {
    final worktree = text(target.worktree['id']);
    final project = objects(host.snapshot['projects'])
        .where((project) => project['id'] == target.worktree['project'])
        .firstOrNull;
    final name = project != null && target.worktree['main'] == true
        ? text(project['name'])
        : worktreeLabel(target.worktree);
    final running = objects(host.snapshot['terminals'])
        .where(
          (terminal) =>
              terminal['worktree'] == worktree &&
              terminalStatus(terminal) == 'running',
        )
        .length;
    final ports = host.ports.entries.length;
    final branch = _git.status?['branch'] as String?;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ProjectBar(
          project: {...?project, 'name': name},
          detail: branch,
          onTap: ModalRoute.canPopOf(context) == true
              ? null
              : () => _workspace(host),
        ),
        GitOverviewCard(
          summary: _git,
          onOpen: () => _open(GitPage(hostId: host.id, worktreeId: worktree)),
        ),
        const SizedBox(height: 12),
        ResourceEntries(
          entries: [
            (
              icon: 'folder',
              label: context.tr('files'),
              detail: null,
              onTap: () =>
                  _open(FilesPage(hostId: host.id, worktreeId: worktree)),
            ),
            (
              icon: 'terminal',
              label: context.tr('terminal'),
              detail: running == 0
                  ? null
                  : context
                        .tr('terminalRunningCount')
                        .replaceAll('{count}', '$running'),
              onTap: () => pushPage(
                context,
                TerminalPage(hostId: host.id, worktreeId: worktree),
              ),
            ),
            (
              icon: 'globe',
              label: context.tr('activityPorts'),
              detail: ports == 0 ? null : '$ports',
              onTap: () => pushPage(context, PortsPage(host: host)),
            ),
          ],
        ),
      ],
    );
  }
}
