import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import 'live.dart';
import 'usage_data.dart';
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
  String _period = 'week';
  bool _weekly = false;
  Map<String, dynamic>? _report;
  String? _error;
  bool _partial = false;
  UsageWatch? _watch;
  int _generation = 0;
  String? _binding;
  AppSession? _session;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _session = AppSession.maybeOf(context);
    final binding = _session?.hosts.map((host) => host.id).join(',');
    if (_binding != binding) {
      _binding = binding;
      _load();
    }
  }

  @override
  void dispose() {
    _generation++;
    final watch = _watch;
    if (watch != null) unawaited(watch.close());
    super.dispose();
  }

  Future<void> _load() async {
    final generation = ++_generation;
    final previous = _watch;
    _watch = null;
    if (previous != null) await previous.close();
    final session = _session;
    if (!mounted ||
        generation != _generation ||
        session == null ||
        session.hosts.isEmpty) {
      return;
    }
    setState(() {
      _report = null;
      _error = null;
      _partial = false;
    });
    final now = DateTime.now().toUtc();
    final end = DateTime.utc(now.year, now.month, now.day + 1);
    final start = end.subtract(Duration(days: _period == 'week' ? 7 : 30));
    final query = <String, dynamic>{
      'start_ms': start.millisecondsSinceEpoch,
      'end_ms': end.millisecondsSinceEpoch,
      'dimension': 'model',
      'projects': [?_project],
      'worktrees': [],
      'providers': [],
      'models': [],
      'before': null,
    };
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
        setState(() {
          _report = host == null
              ? object(view['summary'])
              : view['report'] == null
              ? null
              : object(view['report']);
          _partial = host == null && view['complete'] == false;
          _error = view['error'] == null ? null : tr('settingsLoadFailed');
        });
      }
    } catch (error) {
      if (mounted && generation == _generation) {
        setState(() => _error = failure(error));
      }
    }
  }

  Future<void> _pickHost() async {
    final session = _session;
    if (session == null) return;
    final result = await showAppSheet<String>(
      context,
      tr('selectHost'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              title: Text(tr('allHosts')),
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
      tr('filterProjects'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              title: Text(tr('allProjects')),
              onTap: () => Navigator.pop(context, (_host, null)),
            ),
            for (final host in session.hosts.where(
              (host) => _host == null || host.id == _host,
            ))
              for (final project in objects(host.snapshot['projects']))
                ListTile(
                  title: Text(string(project['name'])),
                  subtitle: Text(host.label),
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
    final colors = Theme.of(context).colorScheme;
    final host = _session?.host(_host);
    final project = objects(
      host?.snapshot['projects'],
    ).where((project) => project['id'] == _project).firstOrNull;
    final totals = object(_report?['totals']);
    final tokens = totals['tokens'] == null
        ? null
        : TokenStack.from(object(totals['tokens']));
    final responses = integer(totals['responses']);
    final cost = object(totals['cost']);
    final days = objects(_report?['days']);
    final buckets = usageBuckets(days, weekly: _weekly);
    final groups = objects(_report?['groups'])
        .map(
          (entry) =>
              entry.containsKey('group') ? object(entry['group']) : entry,
        )
        .toList();
    return PageFrame(
      loading:
          _session?.hosts.isNotEmpty == true &&
          _report == null &&
          _error == null,
      title: tr('usage'),
      failure: _error == null
          ? null
          : FailureState(icon: 'chart', message: _error!, onRetry: _load),
      empty: _session?.hosts.isEmpty != false
          ? EmptyState(icon: 'chart', message: tr('settingsNoHost'))
          : _report != null && responses == 0 && _error == null
          ? EmptyState(icon: 'chart', message: tr('usageEmpty'))
          : null,
      actions: [
        RoundButton(
          icon: 'server',
          tooltip: '${tr('selectHost')}: ${host?.label ?? tr('allHosts')}',
          onPressed: _pickHost,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SelectorCard(
            title: string(project?['name']).isEmpty
                ? tr('allProjects')
                : string(project?['name']),
            subtitle: host?.label ?? tr('allHosts'),
            onTap: _pickProject,
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              for (final period in ['week', 'month'])
                Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: ChoiceChip(
                    label: Text(tr(period)),
                    selected: _period == period,
                    showCheckmark: false,
                    onSelected: (_) {
                      setState(() => _period = period);
                      _load();
                    },
                  ),
                ),
            ],
          ),
          if (_partial)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: Text(tr('settingsUsagePartial')),
            ),
          if (_report != null && responses > 0) ...[
            const SizedBox(height: 12),
            Surface(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    tr('tokens'),
                    style: TextStyle(color: colors.onSurfaceVariant),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    tokens != null
                        ? '${tokens.total}'
                        : responses == 0
                        ? '0'
                        : tr('settingsUsageUnknown'),
                    key: const ValueKey('usage-total'),
                    style: const TextStyle(
                      fontSize: 48,
                      height: 1.1,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 22),
                  Row(
                    children: [
                      Expanded(
                        child: _Metric(
                          value: '$responses',
                          label: tr('requests'),
                        ),
                      ),
                      Expanded(
                        child: _Metric(
                          value: cost.isEmpty
                              ? tr('settingsUsageUnknown')
                              : '\$${(integer(cost['usd_micros']) / 1000000).toStringAsFixed(4)}',
                          label: tr('estimatedCost'),
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 18),
                  Row(
                    children: [
                      for (final weekly in [false, true])
                        Padding(
                          padding: const EdgeInsets.only(right: 8),
                          child: ChoiceChip(
                            key: ValueKey(
                              weekly ? 'usage-weekly' : 'usage-daily',
                            ),
                            label: Text(
                              tr(
                                weekly
                                    ? 'settingsUsageWeekly'
                                    : 'settingsUsageDaily',
                              ),
                            ),
                            selected: _weekly == weekly,
                            showCheckmark: false,
                            onSelected: (_) => setState(() => _weekly = weekly),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  _UsageBars(buckets: buckets),
                  Wrap(
                    spacing: 12,
                    children: [
                      for (final item in [
                        ('settingsUsageCache', colors.secondary),
                        ('settingsUsageInput', colors.tertiary),
                        ('settingsUsageOutput', colors.primary),
                      ])
                        Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            Icon(Icons.circle, size: 8, color: item.$2),
                            const SizedBox(width: 4),
                            Text(tr(item.$1)),
                          ],
                        ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  Text(
                    tr('usageCoverage')
                        .replaceAll('{priced}', '${integer(cost['responses'])}')
                        .replaceAll('{total}', '$responses'),
                    style: TextStyle(
                      fontSize: 14,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 24),
            _SectionTitle(tr('activity'), detail: tr('settingsUtc')),
            const SizedBox(height: 12),
            Surface(
              child: Column(
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [Text(tr('requests')), Text('$responses')],
                  ),
                  const SizedBox(height: 18),
                  LayoutBuilder(
                    builder: (context, constraints) {
                      final size = (constraints.maxWidth - 14 * 4) / 15;
                      final maximum = days.fold<int>(
                        1,
                        (max, day) => math.max(
                          max,
                          integer(object(day['metrics'])['responses']),
                        ),
                      );
                      return Wrap(
                        spacing: 4,
                        runSpacing: 4,
                        children: [
                          for (final day in days)
                            Builder(
                              builder: (context) {
                                final count = integer(
                                  object(day['metrics'])['responses'],
                                );
                                final date =
                                    DateTime.fromMillisecondsSinceEpoch(
                                      integer(day['start_ms']),
                                      isUtc: true,
                                    );
                                return Tooltip(
                                  message: '${usageDate(date)} · $count',
                                  child: Container(
                                    width: size,
                                    height: size,
                                    decoration: BoxDecoration(
                                      borderRadius: BorderRadius.circular(3),
                                      color: count == 0
                                          ? colors.surfaceContainerHigh
                                          : colors.tertiary.withValues(
                                              alpha: .2 + .8 * count / maximum,
                                            ),
                                    ),
                                  ),
                                );
                              },
                            ),
                        ],
                      );
                    },
                  ),
                  if (days.isNotEmpty) ...[
                    const SizedBox(height: 12),
                    Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        for (final day in [days.first, days.last])
                          Text(
                            usageDate(
                              DateTime.fromMillisecondsSinceEpoch(
                                integer(day['start_ms']),
                                isUtc: true,
                              ),
                            ),
                          ),
                      ],
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(height: 24),
            _SectionTitle(tr('modelUsage')),
            const SizedBox(height: 12),
            if (groups.isEmpty)
              EmptyState(icon: 'chart', message: tr('usageEmpty'))
            else
              Surface(
                child: Column(
                  children: [
                    for (final group in groups)
                      Builder(
                        builder: (context) {
                          final name = string(
                            object(object(group['key'])['data'])['model'],
                          );
                          final groupTokens = TokenStack.from(
                            object(object(group['metrics'])['tokens']),
                          );
                          return Padding(
                            padding: const EdgeInsets.symmetric(vertical: 8),
                            child: _ModelShare(
                              name: name,
                              share: (tokens?.total ?? 0) == 0
                                  ? 0
                                  : groupTokens.total / tokens!.total,
                            ),
                          );
                        },
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

class _Metric extends StatelessWidget {
  const _Metric({required this.value, required this.label});
  final String value;
  final String label;
  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(value, style: Theme.of(context).textTheme.titleMedium),
      Text(
        label,
        style: TextStyle(
          color: Theme.of(context).colorScheme.onSurfaceVariant,
          fontSize: 14,
        ),
      ),
    ],
  );
}

class _SectionTitle extends StatelessWidget {
  const _SectionTitle(this.title, {this.detail});
  final String title;
  final String? detail;
  @override
  Widget build(BuildContext context) => Row(
    children: [
      Expanded(
        child: Text(title, style: Theme.of(context).textTheme.titleMedium),
      ),
      if (detail != null)
        Text(
          detail!,
          style: TextStyle(
            fontSize: 14,
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
    ],
  );
}

class _UsageBars extends StatelessWidget {
  const _UsageBars({required this.buckets});
  final List<UsageBucket> buckets;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      height: 140,
      child: BarChart(
        BarChartData(
          minY: 0,
          maxY: buckets.fold<double>(
            1,
            (max, bucket) =>
                math.max(max, (bucket.tokens?.total ?? 0).toDouble()),
          ),
          borderData: FlBorderData(show: false),
          gridData: const FlGridData(show: false),
          barTouchData: BarTouchData(
            touchTooltipData: BarTouchTooltipData(
              fitInsideHorizontally: true,
              fitInsideVertically: true,
              getTooltipItem: (group, _, rod, _) {
                final bucket = buckets[group.x];
                final tokens = bucket.tokens;
                return BarTooltipItem(
                  '${usageDate(bucket.start)}${bucket.end != bucket.start ? '–${usageDate(bucket.end)}' : ''} UTC\n${tokens == null ? tr('settingsUsageUnknown') : '${tr('settingsUsageCache')}: ${tokens.cache}\n${tr('settingsUsageInput')}: ${tokens.input}\n${tr('settingsUsageOutput')}: ${tokens.output}'}',
                  TextStyle(color: colors.onInverseSurface),
                );
              },
            ),
          ),
          titlesData: FlTitlesData(
            leftTitles: const AxisTitles(),
            rightTitles: const AxisTitles(),
            topTitles: const AxisTitles(),
            bottomTitles: AxisTitles(
              sideTitles: SideTitles(
                showTitles: true,
                reservedSize: 30,
                getTitlesWidget: (value, _) {
                  final index = value.toInt();
                  if (index < 0 ||
                      index >= buckets.length ||
                      index % math.max(1, (buckets.length / 5).ceil()) != 0) {
                    return const SizedBox.shrink();
                  }
                  return Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: Text(
                      usageDate(buckets[index].start),
                      style: TextStyle(
                        fontSize: 11,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  );
                },
              ),
            ),
          ),
          barGroups: [
            for (var i = 0; i < buckets.length; i++)
              BarChartGroupData(
                x: i,
                barRods: [
                  BarChartRodData(
                    toY: (buckets[i].tokens?.total ?? 0).toDouble(),
                    width: buckets.length > 10 ? 6 : 20,
                    borderRadius: const BorderRadius.vertical(
                      top: Radius.circular(4),
                    ),
                    rodStackItems: [
                      if (buckets[i].tokens case final tokens?) ...[
                        BarChartRodStackItem(
                          0,
                          tokens.cache.toDouble(),
                          colors.secondary,
                        ),
                        BarChartRodStackItem(
                          tokens.cache.toDouble(),
                          (tokens.cache + tokens.input).toDouble(),
                          colors.tertiary,
                        ),
                        BarChartRodStackItem(
                          (tokens.cache + tokens.input).toDouble(),
                          tokens.total.toDouble(),
                          colors.primary,
                        ),
                      ],
                    ],
                  ),
                ],
              ),
          ],
        ),
        duration: const Duration(milliseconds: 220),
      ),
    );
  }
}

class _ModelShare extends StatelessWidget {
  const _ModelShare({required this.name, required this.share});
  final String name;
  final double share;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Row(
      children: [
        CircleAvatar(
          radius: 16,
          backgroundColor: colors.surfaceContainerHigh,
          foregroundColor: colors.onSurface,
          child: Text(
            name.isEmpty ? '?' : name[0],
            style: const TextStyle(fontSize: 14),
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(name, style: const TextStyle(fontWeight: FontWeight.w600)),
              const SizedBox(height: 6),
              LinearProgressIndicator(
                value: share.clamp(0, 1),
                minHeight: 4,
                borderRadius: BorderRadius.circular(4),
                color: colors.tertiary,
                backgroundColor: colors.surfaceContainerHigh,
              ),
            ],
          ),
        ),
        const SizedBox(width: 16),
        Text(
          '${(share * 100).round()}%',
          style: TextStyle(color: colors.onSurfaceVariant),
        ),
      ],
    );
  }
}
