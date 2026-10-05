import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../runtime/json.dart';
import '../../ui/kit.dart';
import '../terminal/terminal_page.dart';
import 'files_page.dart';
import 'git_page.dart';
import 'workspace.dart';

class LiveResourcesPage extends StatefulWidget {
  const LiveResourcesPage({super.key, this.hostId, this.worktreeId});
  final String? hostId;
  final String? worktreeId;
  @override
  State<LiveResourcesPage> createState() => _LiveResourcesPageState();
}

class _LiveResourcesPageState extends State<LiveResourcesPage> {
  late String? _hostId = widget.hostId;
  late String? _worktreeId = widget.worktreeId;
  String? _scope;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final id = _hostId ?? AppSession.of(context).selectedHost?.id;
    if (_scope != null && _scope != id) _worktreeId = null;
    _scope = id;
  }

  Future<void> _host() async {
    final session = AppSession.of(context);
    final id = await showAppSheet<String>(
      context,
      tr('selectHost'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final host in session.hosts)
              ListTile(
                title: Text(host.label),
                subtitle: host.connected
                    ? null
                    : Text(tr('hostDisconnected')),
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
    final id = await showLiveWorkspacePicker(context, host, _worktreeId);
    if (mounted && id != null) setState(() => _worktreeId = id);
  }

  @override
  Widget build(BuildContext context) {
    final session = AppSession.of(context);
    final host = _hostId == null
        ? session.selectedHost
        : session.host(_hostId!);
    final target = resourceTarget(context, _hostId, _worktreeId);
    return PageFrame(
      title: tr('resources'),
      failure: host?.connected != true ? HostState(added: host != null) : null,
      empty: host?.connected == true && target == null
          ? EmptyState(message: tr('resourceNoWorkspace'))
          : null,
      actions: [
        if (host != null && host.connected && target != null)
          RoundButton(
            icon: 'terminal',
            tooltip: tr('terminal'),
            onPressed: () => pushPage(
              context,
              TerminalPage(
                hostId: host.id,
                worktreeId: text(target.worktree['id']),
              ),
            ),
          ),
        if (ModalRoute.canPopOf(context) != true)
          RoundButton(
            icon: 'server',
            tooltip: tr('selectHost'),
            onPressed: session.hosts.isEmpty ? null : _host,
          ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (host != null && host.connected && target != null) ...[
            SelectorCard(
              title: worktreeLabel(target.worktree),
              subtitle: host.label,
              onTap: ModalRoute.canPopOf(context) == true
                  ? null
                  : () => _workspace(host),
            ),
            const SizedBox(height: 16),
            LayoutBuilder(
              builder: (context, constraints) => Wrap(
                spacing: 10,
                runSpacing: 10,
                children: [
                  for (final item in [('folder', 'files'), ('branch', 'git')])
                    SizedBox(
                      width: (constraints.maxWidth - 10) / 2,
                      child: Surface(
                        onTap: () {
                          final worktree = text(target.worktree['id']);
                          pushPage(context, switch (item.$2) {
                            'files' => FilesPage(
                              hostId: host.id,
                              worktreeId: worktree,
                            ),
                            _ => GitPage(hostId: host.id, worktreeId: worktree),
                          });
                        },
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            AppIcon(
                              item.$1,
                              color: Theme.of(
                                context,
                              ).colorScheme.onSurfaceVariant,
                            ),
                            const SizedBox(height: 22),
                            Text(
                              tr(item.$2),
                              style: Theme.of(context).textTheme.titleMedium,
                            ),
                          ],
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}
