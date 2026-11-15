import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import 'host_parts.dart';
import 'metric_charts.dart';
import 'metric_history.dart';

/// Operating system name and version, such as "macOS 26.1".
String systemLabel(Map<String, dynamic> info) => [
  text(info['os']),
  text(info['os_version']),
].where((part) => part.isNotEmpty).join(' ');

String durationLabel(BuildContext context, num seconds) {
  final (key, count) = durationParts(seconds);
  return context.tr(key).replaceAll('{count}', '$count');
}

/// Host title row, latency and the 2x2 system tiles.
class HostInfoCard extends StatelessWidget {
  const HostInfoCard({
    super.key,
    required this.host,
    required this.metrics,
    required this.history,
  });

  final HostConnection host;
  final Map<String, dynamic> metrics;
  final MetricHistory history;

  @override
  Widget build(BuildContext context) {
    final info = host.info;
    final memory = metrics['memory'] is Map
        ? object(metrics['memory'])
        : object(info['memory']);
    final split = MemorySplit.read(memory);
    final load = object(metrics['load']);
    final coreCount = number(
      info['logical_cpus'],
      (metrics['core_basis_points'] as List?)?.length ?? 0,
    ).toInt();
    final (hostUptime, nodeUptime) = currentUptimes(info, metrics);
    Widget tile(String label, String value, String detail) => Expanded(
      child: InsetCard(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(label, style: dimStyle(context)),
            const SizedBox(height: 2),
            Text(
              value,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: titleStyle,
            ),
            const SizedBox(height: 2),
            Text(
              detail,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: mutedStyle(context),
            ),
          ],
        ),
      ),
    );
    final system = systemLabel(info);
    return Surface(
      padding: const EdgeInsets.fromLTRB(12, 14, 12, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(4, 0, 4, 10),
            child: Row(
              children: [
                Expanded(
                  child: Text(context.tr('hostInfo'), style: titleStyle),
                ),
                RoutePill(route: linkRoute(host.transport)),
              ],
            ),
          ),
          _Latency(current: linkLatency(host.transport), history: history),
          const SizedBox(height: 8),
          Row(
            children: [
              tile(
                context.tr('hostSystem'),
                system.isEmpty ? '—' : system,
                text(info['architecture'], '—'),
              ),
              const SizedBox(width: 8),
              tile(
                context.tr('hostProcessor'),
                coreCount > 0
                    ? context
                          .tr('hostCores')
                          .replaceAll('{count}', '$coreCount')
                    : '—',
                load['one'] is num
                    ? context
                          .tr('hostLoad')
                          .replaceAll('{value}', loadValue(load['one']))
                    : '—',
              ),
            ],
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              tile(
                context.tr('hostMetricMemory'),
                split.total > 0 ? formatBytes(split.total) : '—',
                split.share == null
                    ? '—'
                    : context
                          .tr('hostMemoryShare')
                          .replaceAll('{percent}', percent(split.share!)),
              ),
              const SizedBox(width: 8),
              tile(
                context.tr('hostUptime'),
                info.isEmpty ? '—' : durationLabel(context, hostUptime),
                info.isEmpty
                    ? '—'
                    : context
                          .tr('hostNodeUptime')
                          .replaceAll(
                            '{duration}',
                            durationLabel(context, nodeUptime),
                          ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _Latency extends StatelessWidget {
  const _Latency({required this.current, required this.history});
  final double? current;
  final MetricHistory history;

  @override
  Widget build(BuildContext context) {
    final range = history.latencyRange;
    final values = history.series((sample) => sample.latency);
    final color = Theme.of(context).colorScheme.tertiary;
    return InsetCard(
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(context.tr('hostLatency'), style: dimStyle(context)),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  range == null
                      ? ''
                      : context
                            .tr('hostLatencySummary')
                            .replaceAll('{average}', '${range.$1.round()} ms')
                            .replaceAll('{peak}', '${range.$2.round()} ms'),
                  textAlign: TextAlign.end,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: dimStyle(context),
                ),
              ),
            ],
          ),
          const SizedBox(height: 4),
          Text.rich(
            TextSpan(
              text: current == null ? '—' : '${current!.round()}',
              children: [
                if (current != null)
                  TextSpan(text: ' ms', style: mutedStyle(context, 13)),
              ],
            ),
            style: const TextStyle(
              fontSize: 28,
              height: 1.1,
              fontWeight: FontWeight.w600,
              fontFeatures: figures,
            ),
          ),
          const SizedBox(height: 6),
          AreaChart(
            series: [ChartSeries(values, color)],
            ceiling: math.max(1, (range?.$2 ?? 0) * 1.25),
            height: 30,
          ),
          const SizedBox(height: 4),
        ],
      ),
    );
  }
}

/// Selected link route tinted green for P2P, amber for relay and grey otherwise.
class RoutePill extends StatelessWidget {
  const RoutePill({super.key, required this.route});
  final String? route;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final tone = switch (route) {
      'p2p' => SailryTheme.statusColor(context, StatusTone.success),
      'relay' => SailryTheme.statusColor(context, StatusTone.warning),
      _ => colors.onSurfaceVariant,
    };
    return DecoratedBox(
      decoration: BoxDecoration(
        color: tone.withValues(alpha: .12),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            StatusDot(color: tone, size: 6),
            const SizedBox(width: 6),
            Text(
              context.tr(switch (route) {
                'p2p' => 'connectionRouteP2p',
                'relay' => 'connectionRouteRelay',
                _ => 'connectionRouteUnknown',
              }),
              style: TextStyle(fontSize: 12, color: colors.onSurface),
            ),
          ],
        ),
      ),
    );
  }
}

class StatusDot extends StatelessWidget {
  const StatusDot({super.key, required this.color, this.size = 7});
  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) => Container(
    width: size,
    height: size,
    decoration: BoxDecoration(color: color, shape: BoxShape.circle),
  );
}

Color statusDotColor(BuildContext context, HostStatus status) =>
    switch (status) {
      HostStatus.online => SailryTheme.statusColor(context, StatusTone.success),
      HostStatus.connecting => SailryTheme.statusColor(
        context,
        StatusTone.warning,
      ),
      HostStatus.offline => Theme.of(
        context,
      ).colorScheme.onSurfaceVariant.withValues(alpha: .5),
    };

/// Project, session, terminal and port counts separated by short hairlines.
class CountsCard extends StatelessWidget {
  const CountsCard({super.key, required this.host, required this.onOpen});
  final HostConnection host;

  /// Receives projects, sessions, terminals or ports.
  final ValueChanged<String> onOpen;

  @override
  Widget build(BuildContext context) {
    final divider = Theme.of(context).colorScheme.outlineVariant;
    return Surface(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: ListenableBuilder(
        listenable: host.ports,
        builder: (context, _) {
          final cells = [
            ('projects', 'project', objects(host.snapshot['projects']).length),
            ('sessions', 'chat', objects(host.snapshot['sessions']).length),
            (
              'terminals',
              'terminal',
              objects(host.snapshot['terminals']).length,
            ),
            ('ports', 'activityPorts', host.ports.entries.length),
          ];
          return Row(
            children: [
              for (final (index, (kind, label, count)) in cells.indexed) ...[
                if (index > 0)
                  SizedBox(
                    width: 1,
                    height: 24,
                    child: ColoredBox(color: divider),
                  ),
                Expanded(
                  child: InkWell(
                    key: ValueKey('host-$kind'),
                    borderRadius: BorderRadius.circular(16),
                    onTap: () => onOpen(kind),
                    child: Padding(
                      padding: const EdgeInsets.fromLTRB(4, 12, 4, 10),
                      child: Column(
                        children: [
                          Text(
                            '$count',
                            style: const TextStyle(
                              fontSize: 18,
                              height: 1.3,
                              fontWeight: FontWeight.w600,
                              fontFeatures: figures,
                            ),
                          ),
                          Text(
                            context.tr(label),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: mutedStyle(context),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ],
            ],
          );
        },
      ),
    );
  }
}
