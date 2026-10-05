import 'dart:async';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import 'pair_form.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/project_icon.dart';
import 'resources_page.dart';
import 'projects_page.dart';
import 'ports_page.dart';
import '../conversations/tasks_page.dart';

class LiveHostsPage extends StatefulWidget {
  const LiveHostsPage({super.key, this.onSettings, this.onBrowse});
  final ValueChanged<String>? onSettings;
  final ValueChanged<String>? onBrowse;
  @override
  State<LiveHostsPage> createState() => _LiveHostsPageState();
}

class _LiveHostsPageState extends State<LiveHostsPage> {
  Timer? _timer;
  HostConnection? _host;
  Map<String, dynamic> _metrics = {};
  Object? _error;
  bool _reading = false;
  int _generation = 0;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final host = AppSession.of(context).selectedHost;
    final changed = _host != host;
    _host = host;
    _timer?.cancel();
    if (changed) {
      _generation++;
      _reading = false;
      _metrics = {};
      _error = null;
    }
    if (TickerMode.valuesOf(context).enabled && host?.connected == true) {
      if (changed || _metrics.isEmpty) unawaited(_refresh());
      _timer = Timer.periodic(const Duration(seconds: 5), (_) => _refresh());
    }
  }

  Future<void> _refresh() async {
    final host = _host;
    if (_reading || host == null || !host.connected) return;
    _reading = true;
    final generation = _generation;
    try {
      final response = await host.command('read_host_metrics');
      if (mounted && _host == host && generation == _generation) {
        setState(() {
          _metrics = object(response['data']);
          _error = null;
        });
      }
    } catch (failure) {
      if (mounted && _host == host && generation == _generation) {
        setState(() => _error = failure);
      }
    } finally {
      if (generation == _generation) _reading = false;
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _select() async {
    final session = AppSession.of(context);
    final selected = await showAppSheet<String>(
      context,
      tr('selectHost'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final host in session.hosts)
            ListTile(
              title: Text(host.label),
              subtitle: Text(tr(host.connected ? 'online' : 'offline')),
              trailing: _host == host ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(context, host.id),
            ),
        ],
      ),
    );
    if (selected != null) session.selectHost(selected);
  }

  Future<void> _pair() => showAppSheet<void>(
    context,
    tr('pairTitle'),
    child: PairForm(session: AppSession.of(context)),
  );

  void _browse(String kind) {
    if (kind == 'projects') {
      pushPage(context, ProjectsPage(host: _host!));
    } else if (widget.onBrowse != null) {
      widget.onBrowse!(kind);
    } else {
      pushPage(
        context,
        TasksPage(
          initialFilter: kind == 'terminals' ? 'terminal' : 'all',
          sessionsOnly: kind == 'sessions',
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = AppSession.of(context);
    final host = session.selectedHost;
    final colors = Theme.of(context).colorScheme;
    final memory = object(_metrics['memory']);
    final disks = objects(_metrics['disks']);
    final disk = disks.firstOrNull ?? <String, dynamic>{};
    double? used(Map<String, dynamic> value) {
      final total = number(value['total_bytes']);
      return total > 0
          ? (total - number(value['available_bytes'])) / total
          : null;
    }

    final cpu = _metrics['cpu_basis_points'];
    return PageFrame(
      loading: session.loading,
      title: tr('hosts'),
      failure: !session.ready && session.error != null
          ? FailureState(message: tr('startupFailed'), onRetry: session.start)
          : host?.connected != true && !session.loading
          ? HostState(
              added: host != null,
              action: host == null
                  ? FilledButton(
                      onPressed: session.ready ? _pair : null,
                      child: Text(tr('pair')),
                    )
                  : null,
            )
          : _error != null
          ? FailureState(
              icon: 'cpu',
              message: tr('hostMetricsFailed'),
              onRetry: _refresh,
            )
          : null,
      actions: [
        if (host != null && host.connected)
          RoundButton(
            icon: 'link',
            tooltip: tr('ports'),
            onPressed: () => pushPage(context, PortsPage(host: host)),
          ),
        if (session.hosts.isNotEmpty)
          RoundButton(
            icon: 'server',
            tooltip: tr('selectHost'),
            onPressed: _select,
          ),
        RoundButton(icon: 'plus', tooltip: tr('pair'), onPressed: _pair),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (host != null) ...[
            Surface(
              onTap: () => widget.onSettings?.call(host.id),
              child: Column(
                children: [
                  Row(
                    children: [
                      const Surface(
                        radius: 14,
                        child: AppIcon('server', size: 26),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              host.label,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.titleMedium,
                            ),
                            const SizedBox(height: 4),
                            Row(
                              children: [
                                Expanded(
                                  child: Text(
                                    [
                                      text(host.info['os']),
                                      text(host.info['architecture']),
                                    ].where((s) => s.isNotEmpty).join(' · '),
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      color: colors.onSurfaceVariant,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Icon(
                                  Icons.circle,
                                  size: 6,
                                  color: host.connected
                                      ? colors.tertiary
                                      : colors.onSurfaceVariant,
                                ),
                                const SizedBox(width: 5),
                                Text(
                                  tr(host.connected ? 'online' : 'offline'),
                                  style: Theme.of(context).textTheme.bodySmall,
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            Surface(
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceAround,
                children: [
                  _Gauge(
                    label: tr('hostMetricCpu'),
                    value: cpu is num ? cpu / 10000 : null,
                  ),
                  _Gauge(label: tr('hostMetricMemory'), value: used(memory)),
                  _Gauge(label: tr('hostMetricDisk'), value: used(disk)),
                ],
              ),
            ),
            const SizedBox(height: 12),
            Surface(
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceAround,
                children: [
                  for (final entry in [
                    ('projects', 'project'),
                    ('sessions', 'chat'),
                    ('terminals', 'terminal'),
                  ])
                    Expanded(
                      child: InkWell(
                        key: ValueKey('host-${entry.$1}'),
                        borderRadius: BorderRadius.circular(12),
                        onTap: () => _browse(entry.$1),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(vertical: 4),
                          child: Column(
                            children: [
                              Text(
                                '${objects(host.snapshot[entry.$1]).length}',
                                style: Theme.of(context).textTheme.titleLarge,
                              ),
                              Text(tr(entry.$2)),
                            ],
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            if (objects(host.snapshot['projects']).isNotEmpty)
              Surface(
                child: Column(
                  children: [
                    for (final project in objects(host.snapshot['projects']))
                      ListTile(
                        contentPadding: EdgeInsets.zero,
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
                          pushPage(
                            context,
                            ResourcesPage(
                              hostId: host.id,
                              worktreeId: text(tree?['id']),
                            ),
                          );
                        },
                      ),
                  ],
                ),
              ),
            const SizedBox(height: 12),
            if (objects(_metrics['processes']).isEmpty)
              EmptyState(icon: 'cpu', message: tr('hostProcessesEmpty'))
            else
              Surface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    for (final process in objects(
                      _metrics['processes'],
                    ).take(12))
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 6),
                        child: Row(
                          children: [
                            Expanded(
                              child: Text(
                                text(process['name']),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                            const SizedBox(width: 8),
                            SizedBox(
                              width: 64,
                              child: Text(
                                process['cpu_basis_points'] is num
                                    ? '${(number(process['cpu_basis_points']) / 100).toStringAsFixed(1)}%'
                                    : '—',
                                textAlign: TextAlign.right,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                            const SizedBox(width: 12),
                            SizedBox(
                              width: 80,
                              child: Text(
                                '${(number(process['memory_bytes']) / 1048576).toStringAsFixed(0)} MB',
                                textAlign: TextAlign.right,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ],
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

class _Gauge extends StatelessWidget {
  const _Gauge({required this.label, required this.value});
  final String label;
  final double? value;
  @override
  Widget build(BuildContext context) => Column(
    children: [
      SizedBox.square(
        dimension: 64,
        child: Stack(
          alignment: Alignment.center,
          children: [
            SizedBox.expand(
              child: CircularProgressIndicator(
                value: value?.clamp(0, 1) ?? 0,
                backgroundColor: Theme.of(context).colorScheme.outlineVariant,
                color: Theme.of(context).colorScheme.tertiary,
                semanticsLabel: label,
              ),
            ),
            Text(value == null ? '—' : '${(value! * 100).toStringAsFixed(0)}%'),
          ],
        ),
      ),
      const SizedBox(height: 10),
      Text(label),
    ],
  );
}
