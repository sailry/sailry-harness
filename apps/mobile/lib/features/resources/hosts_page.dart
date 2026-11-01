import 'dart:async';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import 'pair_form.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import 'resources_page.dart';
import 'projects_page.dart';
import 'ports_page.dart';
import '../conversations/tasks_page.dart';
import 'host/host_info_card.dart';
import 'host/host_sheet.dart';
import 'host/host_tabs.dart';
import 'host/metric_history.dart';
import 'host/resource_cards.dart';

class HostsPage extends StatefulWidget {
  const HostsPage({super.key, this.onBrowse});
  final ValueChanged<String>? onBrowse;
  @override
  State<HostsPage> createState() => _HostsPageState();
}

class _HostsPageState extends State<HostsPage> {
  Timer? _timer;
  HostConnection? _host;
  Map<String, dynamic> _metrics = {};
  final _history = MetricHistory();
  Object? _error;
  bool _reading = false;
  int _generation = 0;
  HostTab _tab = HostTab.projects;
  bool _byMemory = false;
  String _processQuery = '';
  String _projectQuery = '';

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final host = AppSession.of(context).selectedHost;
    final changed = _host != host;
    _host = host;
    if (changed) {
      _timer?.cancel();
      _timer = null;
      _generation++;
      _reading = false;
      _metrics = {};
      _history.clear();
      _error = null;
      _processQuery = '';
      _projectQuery = '';
    }
    if (!TickerMode.valuesOf(context).enabled || host?.connected != true) {
      _timer?.cancel();
      _timer = null;
      return;
    }
    // Session updates must not restart the interval and starve sampling.
    if (_timer != null) return;
    _timer = Timer.periodic(samplePeriod, (_) => _refresh());
    unawaited(_refresh());
  }

  Future<void> _refresh({bool retry = false}) async {
    final host = _host;
    if (_reading || host == null || !host.connected) return;
    _reading = true;
    final generation = _generation;
    var again = false;
    try {
      final response = await host.command('read_host_metrics', {
        'detailed': true,
      });
      if (mounted && _host == host && generation == _generation) {
        final metrics = object(response['data']);
        // Node only reports rates when its previous detailed read is under
        // five seconds old. After a pause, read again shortly instead of
        // charting a gap.
        again = !retry && metrics['network'] is! Map;
        if (!again || _metrics.isEmpty) {
          setState(() {
            _metrics = metrics;
            if (!again) {
              _history.add(
                MetricSample.read(metrics, linkLatency(host.transport)),
              );
            }
            _error = null;
          });
        }
      }
    } catch (failure) {
      if (mounted && _host == host && generation == _generation) {
        setState(() => _error = failure);
      }
    } finally {
      if (generation == _generation) _reading = false;
    }
    if (again) {
      await Future<void>.delayed(const Duration(milliseconds: 1100));
      if (mounted && _timer != null && generation == _generation) {
        await _refresh(retry: true);
      }
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _pair() => showAppSheet<void>(
    context,
    context.tr('pairTitle'),
    child: PairForm(session: AppSession.of(context)),
  );

  void _select() => showHostSheet(context, onPair: _pair);

  void _browse(String kind) {
    final host = _host!;
    if (kind == 'ports') {
      pushPage(context, PortsPage(host: host));
    } else if (kind == 'projects') {
      pushPage(context, ProjectsPage(host: host));
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

  void _openProject(HostConnection host, Map<String, dynamic> project) {
    final tree = objects(
      host.snapshot['worktrees'],
    ).where((tree) => tree['project'] == project['id']).firstOrNull;
    pushPage(
      context,
      ResourcesPage(hostId: host.id, worktreeId: text(tree?['id'])),
    );
  }

  /// Keeps the host switcher above a state so another host stays reachable.
  Widget _blocked(HostConnection host, Widget state) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      HostBar(host: host, onTap: _select),
      Expanded(
        child: Padding(
          padding: EdgeInsets.only(
            bottom: 24 + MediaQuery.paddingOf(context).bottom,
          ),
          child: state,
        ),
      ),
    ],
  );

  @override
  Widget build(BuildContext context) {
    final session = AppSession.of(context);
    final host = session.selectedHost;
    final startupFailed = !session.ready && session.error != null;
    return PageFrame(
      loading: session.loading,
      title: context.tr('hosts'),
      scroll: false,
      failure: startupFailed
          ? FailureState(
              message: context.tr('startupFailed'),
              onRetry: session.start,
            )
          : host == null && !session.loading
          ? HostState(
              added: false,
              action: FilledButton(
                onPressed: session.ready ? _pair : null,
                child: Text(context.tr('pair')),
              ),
            )
          : null,
      actions: [
        RoundButton(
          icon: 'plus',
          tooltip: context.tr('pair'),
          onPressed: _pair,
        ),
      ],
      child: host == null
          ? const SizedBox.shrink()
          : !host.connected
          ? session.loading
                ? const SizedBox.shrink()
                : _blocked(host, const HostState(added: true))
          : _error != null
          ? _blocked(
              host,
              FailureState(
                icon: 'cpu',
                message: context.tr('hostMetricsFailed'),
                onRetry: _refresh,
              ),
            )
          : _content(host),
    );
  }

  Widget _content(HostConnection host) {
    final projects = objects(host.snapshot['projects']);
    final sessions = objects(host.snapshot['sessions']);
    final processes = objects(_metrics['processes']);
    const gap = SizedBox(height: 12);
    return CustomScrollView(
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      slivers: [
        SliverToBoxAdapter(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              HostBar(host: host, onTap: _select),
              const SizedBox(height: 14),
              HostInfoCard(host: host, metrics: _metrics, history: _history),
              gap,
              CountsCard(host: host, onOpen: _browse),
              if (_metrics.isNotEmpty) ...[
                gap,
                TrendCard(history: _history),
                gap,
                CpuCard(metrics: _metrics),
                gap,
                UsageRings(metrics: _metrics),
                if (_metrics['network'] is Map) ...[
                  gap,
                  NetworkCard(metrics: _metrics, history: _history),
                ],
                if (objects(_metrics['disks']).isNotEmpty) ...[
                  gap,
                  DiskCard(metrics: _metrics, history: _history),
                ],
              ],
              const SizedBox(height: 18),
            ],
          ),
        ),
        SliverPersistentHeader(
          pinned: true,
          delegate: HostTabsHeader(
            tab: _tab,
            projects: projects.length,
            processes: number(
              _metrics['process_count'],
              processes.length,
            ).toInt(),
            byMemory: _byMemory,
            onTab: (tab) => setState(() {
              _tab = tab;
              _processQuery = '';
              _projectQuery = '';
            }),
            onSort: (memory) => setState(() => _byMemory = memory),
          ),
        ),
        SliverPadding(
          padding: EdgeInsets.only(
            bottom: 24 + MediaQuery.paddingOf(context).bottom,
          ),
          sliver: SliverToBoxAdapter(
            child: switch (_tab) {
              HostTab.processes => ProcessList(
                processes: processes,
                query: _processQuery,
                byMemory: _byMemory,
                onQuery: (value) => setState(() => _processQuery = value),
              ),
              HostTab.projects => ProjectList(
                projects: projects,
                sessions: sessions,
                query: _projectQuery,
                onQuery: (value) => setState(() => _projectQuery = value),
                onOpen: (project) => _openProject(host, project),
              ),
            },
          ),
        ),
      ],
    );
  }
}
