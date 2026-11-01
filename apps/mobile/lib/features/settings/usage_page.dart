import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../conversations/live/project_bar.dart';
import 'live.dart';
import 'usage_data.dart';
import 'usage_sections.dart';
import 'usage_watch.dart';

class UsagePage extends StatefulWidget {
  const UsagePage({super.key, this.watch = UsageWatch.open});
  final UsageWatchFactory watch;
  @override
  State<UsagePage> createState() => _UsagePageState();
}

class _UsagePageState extends State<UsagePage> {
  String? _host;
  String? _project;
  UsageRange _range = UsageRange.month;
  UsageBreakdown _breakdown = UsageBreakdown.model;

  /// Latest grouping the shown report can render while a regrouped report loads.
  UsageBreakdown _fallback = UsageBreakdown.model;
  bool _expanded = false;

  /// Query of the active subscription.
  Map<String, dynamic>? _query;
  Map<String, dynamic>? _report;

  /// Query and range the shown report was read with.
  Map<String, dynamic>? _shown;
  UsageRange _shownRange = UsageRange.month;
  String? _error;

  /// Set only after the combined view stays incomplete, so hosts reporting
  /// one after another do not make the note flicker.
  bool _partial = false;
  Timer? _settling;
  UsageWatch? _watch;
  int _generation = 0;
  String? _binding;
  AppSession? _session;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _session = AppSession.maybeOf(context);
    final binding = _session?.hosts
        .map((host) => '${host.id}:${host.connected}')
        .join(',');
    if (_binding != binding) {
      _binding = binding;
      _load();
    }
  }

  @override
  void dispose() {
    _generation++;
    _settling?.cancel();
    final watch = _watch;
    if (watch != null) unawaited(watch.close());
    super.dispose();
  }

  /// Opens a subscription for the current choices. [keep] retains the shown
  /// report until the new one arrives, for range and grouping changes.
  Future<void> _load({bool keep = false}) async {
    final generation = ++_generation;
    _settling?.cancel();
    _settling = null;
    final previous = _watch;
    _watch = null;
    if (previous != null) await previous.close();
    final session = _session;
    if (!mounted || generation != _generation) return;
    final range = _range;
    final query = usageQuery(
      range,
      DateTime.now(),
      dimension: usageDimension(_breakdown),
      project: _project,
    );
    setState(() {
      _query = query;
      if (!keep) {
        _report = null;
        _shown = null;
      }
      _error = null;
      _partial = false;
    });
    if (session == null ||
        session.hosts.isEmpty ||
        (_host == null
            ? !session.hosts.any((host) => host.connected)
            : session.host(_host)?.connected != true)) {
      return;
    }
    try {
      final host = session.host(_host);
      final watch = await widget.watch(session, host, query);
      if (!mounted || generation != _generation) {
        await watch.close();
        return;
      }
      _watch = watch;
      while (mounted && generation == _generation) {
        final view = object(jsonDecode(await watch.next()));
        if (!mounted || generation != _generation) break;
        final report = view[host == null ? 'summary' : 'report'];
        setState(() {
          if (report is Map<String, dynamic>) {
            _report = report;
            _shown = query;
            _shownRange = range;
          }
          _error = view['error'] == null
              ? null
              : context.tr('settingsLoadFailed');
        });
        if (host == null) _settle(view['complete'] != false, generation);
      }
    } catch (error) {
      if (mounted && generation == _generation) {
        setState(() => _error = failure(error, translate: context.tr));
      }
    }
  }

  void _settle(bool complete, int generation) {
    if (complete) {
      _settling?.cancel();
      _settling = null;
      if (_partial) setState(() => _partial = false);
      return;
    }
    if (_partial || _settling != null) return;
    _settling = Timer(const Duration(milliseconds: 1500), () {
      _settling = null;
      if (mounted && generation == _generation) {
        setState(() => _partial = true);
      }
    });
  }

  void _selectRange(UsageRange range) {
    if (range == _range) return;
    setState(() => _range = range);
    _load(keep: true);
  }

  void _group(UsageBreakdown breakdown) {
    if (breakdown == _breakdown) return;
    if (usageDimension(_breakdown) == _shown?['dimension']) {
      _fallback = _breakdown;
    }
    setState(() {
      _breakdown = breakdown;
      _expanded = false;
    });
    if (usageDimension(breakdown) != _query?['dimension']) _load(keep: true);
  }

  Future<void> _pickHost() async {
    final session = _session;
    if (session == null) return;
    final result = await showAppSheet<String>(
      context,
      context.tr('selectHost'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              title: Text(context.tr('allHosts')),
              trailing: _host == null ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(context, ''),
            ),
            for (final host in session.hosts)
              ListTile(
                title: Text(host.label),
                trailing: _host == host.id ? const AppIcon('check') : null,
                onTap: () => Navigator.pop(context, host.id),
              ),
          ],
        ),
      ),
    );
    if (!mounted || result == null) return;
    setState(() {
      _host = result.isEmpty ? null : result;
      _project = null;
    });
    await _load();
  }

  Future<void> _pickProject() async {
    final session = _session;
    if (session == null) return;
    final result = await showAppSheet<(String?, String?)>(
      context,
      context.tr('filterProjects'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              title: Text(context.tr('allProjects')),
              onTap: () => Navigator.pop(context, (_host, null)),
            ),
            for (final host in session.hosts.where(
              (host) => _host == null || host.id == _host,
            ))
              for (final project in objects(host.snapshot['projects']))
                ListTile(
                  title: Text(string(project['name'])),
                  subtitle: Text(host.label),
                  trailing: _project == project['id']
                      ? const AppIcon('check')
                      : null,
                  onTap: () =>
                      Navigator.pop(context, (host.id, string(project['id']))),
                ),
          ],
        ),
      ),
    );
    if (!mounted || result == null) return;
    setState(() {
      _host = result.$1;
      _project = result.$2;
    });
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final session = _session;
    final host = session?.host(_host);
    final project = objects(
      host?.snapshot['projects'],
    ).where((project) => project['id'] == _project).firstOrNull;
    final report = _report;
    final overview = UsageOverview(object(report?['totals']));
    final added = session?.hosts.isNotEmpty == true;
    final connected = _host == null
        ? session?.hosts.any((host) => host.connected) == true
        : host?.connected == true;
    final shown = _shown;
    final query = _query;
    // The shown report covers another window until the new range arrives.
    final moving =
        shown != null &&
        query != null &&
        (shown['start_ms'] != query['start_ms'] ||
            shown['end_ms'] != query['end_ms']);
    final regrouping =
        shown != null && shown['dimension'] != usageDimension(_breakdown);
    final rows = report == null
        ? const <UsageRow>[]
        : usageRows(
            objects(report['groups']),
            regrouping ? _fallback : _breakdown,
            projects: {
              for (final source in session?.hosts ?? const <HostConnection>[])
                for (final item in objects(source.snapshot['projects']))
                  string(item['id']): string(item['name']),
            },
            unassigned: context.tr('usageUnassigned'),
          );
    return PageFrame(
      loading: connected && report == null && _error == null,
      title: context.tr('usage'),
      failure: !connected
          ? HostState(added: added)
          : _error == null
          ? null
          : FailureState(icon: 'chart', message: _error!, onRetry: _load),
      empty:
          connected &&
              report != null &&
              overview.responses == 0 &&
              _error == null
          ? EmptyState(icon: 'chart', message: context.tr('usageEmpty'))
          : null,
      actions: [
        RoundButton(
          icon: 'server',
          tooltip:
              '${context.tr('selectHost')}: ${host?.label ?? context.tr('allHosts')}',
          onPressed: added ? _pickHost : null,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ProjectBar(
            project: project,
            detail: host?.label ?? context.tr('allHosts'),
            onTap: _pickProject,
          ),
          const SizedBox(height: 10),
          UsageSegments<UsageRange>(
            key: const ValueKey('usage-range'),
            values: UsageRange.values,
            selected: _range,
            label: (range) => context.tr(range.label),
            onChanged: _selectRange,
          ),
          if (report != null && overview.responses > 0) ...[
            const SizedBox(height: 20),
            AnimatedOpacity(
              opacity: moving ? .55 : 1,
              duration: const Duration(milliseconds: 150),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  UsageSummary(overview: overview, stale: _partial),
                  const SizedBox(height: 28),
                  UsageTrend(
                    report: report,
                    range: _shownRange,
                    offset: integer(shown?['utc_offset_minutes']),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 28),
            UsageHeading(
              context.tr('usageBreakdown'),
              trailing: SizedBox(
                width: 200,
                child: UsageSegments<UsageBreakdown>(
                  key: const ValueKey('usage-breakdown'),
                  values: UsageBreakdown.values,
                  selected: _breakdown,
                  label: (breakdown) => switch (breakdown) {
                    UsageBreakdown.model => context.tr('model'),
                    UsageBreakdown.provider => context.tr('usageProvider'),
                    UsageBreakdown.project => context.tr('project'),
                  },
                  onChanged: _group,
                ),
              ),
            ),
            AnimatedOpacity(
              opacity: moving || regrouping ? .55 : 1,
              duration: const Duration(milliseconds: 150),
              child: UsageRows(
                rows: rows,
                expanded: _expanded,
                onToggle: () => setState(() => _expanded = !_expanded),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
