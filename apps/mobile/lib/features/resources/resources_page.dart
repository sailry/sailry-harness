import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../runtime/session.dart';
import 'live_resources.dart';
import 'files_page.dart';
import 'git_page.dart';
import 'workspace.dart';

class ResourcesPage extends StatefulWidget {
  const ResourcesPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.host = 'Studio',
    this.project = 'sailry-web',
    this.branch = 'feature/sign-in',
  });

  final String host;
  final String? hostId;
  final String? worktreeId;
  final String project;
  final String branch;

  @override
  State<ResourcesPage> createState() => _ResourcesPageState();
}

class _ResourcesPageState extends State<ResourcesPage> {
  late String _host = widget.host;
  late String _project = widget.project;
  late String _branch = widget.branch;
  final Set<String> _closedPorts = {};

  Future<void> _pickWorkspace() async {
    final value = await showWorkspacePicker(
      context,
      host: _host,
      project: _project,
      branch: _branch,
    );
    if (value != null && mounted) {
      setState(() {
        _project = value.project;
        _branch = value.branch;
      });
    }
  }

  String get _workspaceKey => '$_host/$_project/$_branch';

  void _showPorts() {
    showAppSheet<void>(
      context,
      tr('ports'),
      child: StatefulBuilder(
        builder: (context, update) => Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            if (_closedPorts.contains(_workspaceKey))
              Text(tr('portClosed'))
            else ...[
              Surface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Web / 5173',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: 12),
                    Text('${tr('portTarget')}: $_host :5173'),
                    Text('${tr('localPort')}: 127.0.0.1:5173'),
                  ],
                ),
              ),
              const SizedBox(height: 16),
              Text(
                tr('portNote'),
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 16),
              OutlinedButton(
                onPressed: () {
                  setState(() => _closedPorts.add(_workspaceKey));
                  update(() {});
                },
                child: Text(tr('closePort')),
              ),
            ],
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    if (AppSession.maybeOf(context) != null) {
      return LiveResourcesPage(
        hostId: widget.hostId,
        worktreeId: widget.worktreeId,
      );
    }
    final entries = [
      (
        'folder',
        'files',
        'browseFiles',
        () => pushPage(
          context,
          FilesPage(host: _host, project: _project, branch: _branch),
        ),
      ),
      (
        'branch',
        'git',
        'gitSummary',
        () => pushPage(
          context,
          GitPage(host: _host, project: _project, branch: _branch),
        ),
      ),
      ('link', 'ports', 'portsSub', _showPorts),
    ];
    return PageFrame(
      title: tr('resources'),
      actions: [
        if (ModalRoute.canPopOf(context) != true)
          RoundButton(
            icon: 'server',
            tooltip: '${tr('selectHost')}: $_host',
            onPressed: () => showHostPicker(
              context,
              selected: _host,
              onSelected: (host) => setState(() {
                _host = host;
                _project = host == 'Build Server' ? 'sailry-api' : 'sailry-web';
                _branch = 'feature/sign-in';
              }),
            ),
          ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SelectorCard(
            title: _project,
            subtitle: _branch,
            onTap: ModalRoute.canPopOf(context) == true ? null : _pickWorkspace,
          ),
          const SizedBox(height: 16),
          LayoutBuilder(
            builder: (context, constraints) => Wrap(
              spacing: 10,
              runSpacing: 10,
              children: [
                for (final entry in entries)
                  SizedBox(
                    width: (constraints.maxWidth - 10) / 2,
                    child: Surface(
                      onTap: entry.$4,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          AppIcon(
                            entry.$1,
                            color: Theme.of(
                              context,
                            ).colorScheme.onSurfaceVariant,
                          ),
                          const SizedBox(height: 22),
                          Text(
                            tr(entry.$2),
                            style: Theme.of(context).textTheme.titleMedium,
                          ),
                          const SizedBox(height: 4),
                          Text(
                            tr(entry.$3).replaceAll('Studio', _host),
                            style: Theme.of(context).textTheme.bodyMedium
                                ?.copyWith(
                                  color: Theme.of(
                                    context,
                                  ).colorScheme.onSurfaceVariant,
                                ),
                          ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
