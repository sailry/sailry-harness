import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'host_parts.dart';
import 'metric_charts.dart';
import 'metric_history.dart';

const _cardPadding = EdgeInsets.fromLTRB(16, 14, 16, 14);

(String, String) _axis(BuildContext context) =>
    (context.tr('hostChartStart'), context.tr('hostChartNow'));

(String, String) _rateGrid(double ceiling) =>
    (formatRate(ceiling / 2), formatRate(ceiling));

/// CPU, memory and GPU utilization over the retained minute.
class TrendCard extends StatelessWidget {
  const TrendCard({super.key, required this.history});
  final MetricHistory history;

  @override
  Widget build(BuildContext context) {
    final samples = history.samples;
    final latest = samples.lastOrNull;
    final gpu = samples.any((sample) => sample.gpu != null);
    final cpuColor = metricColor(context, MetricTone.cpu);
    final memoryColor = metricColor(context, MetricTone.memory);
    final gpuColor = metricColor(context, MetricTone.gpu);
    String value(double? share) => share == null ? '—' : percent(share);
    return Surface(
      padding: _cardPadding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          CardHeader(
            title: context.tr('hostTrend'),
            detail: context.tr('hostTrendWindow'),
          ),
          AreaChart(
            height: 126,
            ceiling: 1,
            gridLabels: ('50%', '100%'),
            axisLabels: _axis(context),
            series: [
              ChartSeries(
                history.series((sample) => sample.memory),
                memoryColor,
              ),
              ChartSeries(history.series((sample) => sample.cpu), cpuColor),
              if (gpu)
                ChartSeries(history.series((sample) => sample.gpu), gpuColor),
            ],
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 16,
            runSpacing: 6,
            children: [
              LegendEntry(
                color: cpuColor,
                label: context.tr('hostMetricCpu'),
                value: value(latest?.cpu),
              ),
              LegendEntry(
                color: memoryColor,
                label: context.tr('hostMetricMemory'),
                value: value(latest?.memory),
              ),
              if (gpu)
                LegendEntry(
                  color: gpuColor,
                  label: context.tr('hostMetricGpu'),
                  value: value(latest?.gpu),
                ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Total CPU, load averages and the per-core grid.
class CpuCard extends StatelessWidget {
  const CpuCard({super.key, required this.metrics});
  final Map<String, dynamic> metrics;

  @override
  Widget build(BuildContext context) {
    final cpu = metrics['cpu_basis_points'];
    final load = object(metrics['load']);
    final cores = (metrics['core_basis_points'] as List? ?? const [])
        .whereType<num>()
        .map((value) => (value / 10000).clamp(0, 1).toDouble())
        .toList();
    final averages = [
      load['one'],
      load['five'],
      load['fifteen'],
    ].whereType<num>().map(loadValue).join('  ');
    return Surface(
      padding: _cardPadding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          CardHeader(
            title: context.tr('hostMetricCpu'),
            value: cpu is num ? percent(cpu / 10000) : '—',
            detail: averages.isEmpty
                ? null
                : context.tr('hostLoad').replaceAll('{value}', averages),
          ),
          if (cores.isNotEmpty) ...[
            const SizedBox(height: 2),
            CoreGrid(shares: cores),
            const SizedBox(height: 8),
            Text(
              context
                  .tr('hostCoreHint')
                  .replaceAll('{count}', '${cores.length}'),
              style: dimStyle(context),
            ),
          ],
        ],
      ),
    );
  }
}

/// Memory and GPU rings side by side; memory takes the row without a GPU.
class UsageRings extends StatelessWidget {
  const UsageRings({super.key, required this.metrics});
  final Map<String, dynamic> metrics;

  @override
  Widget build(BuildContext context) {
    final gpu = objects(metrics['gpus']).firstOrNull;
    final memory = _MemoryRing(metrics: metrics);
    if (gpu == null) return memory;
    return IntrinsicHeight(
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(child: memory),
          const SizedBox(width: 10),
          Expanded(child: _GpuRing(gpu: gpu)),
        ],
      ),
    );
  }
}

class _RingCard extends StatelessWidget {
  const _RingCard({
    required this.title,
    required this.ring,
    required this.legend,
    required this.dots,
  });
  final String title;
  final Widget ring;
  final Widget legend;
  final Widget dots;

  @override
  Widget build(BuildContext context) => Surface(
    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 14),
    child: Column(
      children: [
        Align(
          alignment: AlignmentDirectional.centerStart,
          child: Text(title, style: titleStyle),
        ),
        const SizedBox(height: 8),
        ring,
        const SizedBox(height: 8),
        legend,
        const SizedBox(height: 8),
        SizedBox(height: 4, child: dots),
      ],
    ),
  );
}

Widget _ringCenter(BuildContext context, String value, String detail) =>
    Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            value,
            style: const TextStyle(
              fontSize: 17,
              height: 1.2,
              fontWeight: FontWeight.w600,
              fontFeatures: figures,
            ),
          ),
          Text(
            detail,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.center,
            style: dimStyle(context, 10.5),
          ),
        ],
      ),
    );

/// Tapping the ring cycles the single legend line through used, cache and swap.
class _MemoryRing extends StatefulWidget {
  const _MemoryRing({required this.metrics});
  final Map<String, dynamic> metrics;

  @override
  State<_MemoryRing> createState() => _MemoryRingState();
}

class _MemoryRingState extends State<_MemoryRing> {
  int _item = 0;

  @override
  Widget build(BuildContext context) {
    final split = MemorySplit.read(object(widget.metrics['memory']));
    final swap = object(widget.metrics['swap']);
    final color = metricColor(context, MetricTone.memory);
    final cache = color.withValues(alpha: .45);
    final colors = Theme.of(context).colorScheme;
    final share = split.share;
    final swapTotal = number(swap['total_bytes']);
    final (label, value, mark) = switch (_item) {
      0 => ('hostMemoryUsed', formatBytes(split.used), color),
      1 => ('hostMemoryCache', formatBytes(split.cache), cache),
      _ => (
        'hostMemorySwap',
        swapTotal > 0 ? byteRatio(number(swap['used_bytes']), swapTotal) : '—',
        chartTrack(context),
      ),
    };
    return _RingCard(
      title: context.tr('hostMetricMemory'),
      ring: Semantics(
        button: true,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => setState(() => _item = (_item + 1) % 3),
          child: RingChart(
            segments: [
              RingSegment(share ?? 0, levelColor(context, share ?? 0, color)),
              RingSegment(split.cacheShare ?? 0, cache),
            ],
            child: _ringCenter(
              context,
              share == null ? '—' : percent(share),
              split.total > 0 ? byteRatio(split.used, split.total) : '',
            ),
          ),
        ),
      ),
      legend: LegendEntry(color: mark, label: context.tr(label), value: value),
      dots: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        spacing: 4,
        children: [
          for (var index = 0; index < 3; index++)
            Container(
              width: 4,
              height: 4,
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: index == _item
                    ? colors.onSurface
                    : colors.onSurfaceVariant.withValues(alpha: .25),
              ),
            ),
        ],
      ),
    );
  }
}

class _GpuRing extends StatelessWidget {
  const _GpuRing({required this.gpu});
  final Map<String, dynamic> gpu;

  @override
  Widget build(BuildContext context) {
    final usage = gpu['usage_basis_points'];
    final share = usage is num ? (usage / 10000).clamp(0, 1).toDouble() : null;
    final color = metricColor(context, MetricTone.gpu);
    final value = share == null ? '—' : percent(share);
    return _RingCard(
      title: context.tr('hostMetricGpu'),
      ring: RingChart(
        segments: [
          if (share != null)
            RingSegment(share, levelColor(context, share, color)),
        ],
        child: _ringCenter(context, value, text(gpu['name'])),
      ),
      legend: LegendEntry(
        color: color,
        label: context.tr('hostGpuUsage'),
        value: value,
      ),
      dots: const SizedBox.shrink(),
    );
  }
}

/// Download and upload rates with interface totals since boot.
class NetworkCard extends StatelessWidget {
  const NetworkCard({super.key, required this.metrics, required this.history});
  final Map<String, dynamic> metrics;
  final MetricHistory history;

  @override
  Widget build(BuildContext context) {
    final network = object(metrics['network']);
    final latest = history.samples.lastOrNull;
    final received = history.series((sample) => sample.received);
    final sent = history.series((sample) => sample.sent);
    final ceiling = rateCeiling([...received, ...sent]);
    final down = metricColor(context, MetricTone.received);
    final up = metricColor(context, MetricTone.sent);
    return Surface(
      padding: _cardPadding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          CardHeader(
            title: context.tr('hostNetwork'),
            detail: network.isEmpty
                ? null
                : context
                      .tr('hostNetworkTotals')
                      .replaceAll(
                        '{received}',
                        formatBytes(number(network['received_bytes'])),
                      )
                      .replaceAll(
                        '{sent}',
                        formatBytes(number(network['transmitted_bytes'])),
                      ),
          ),
          AreaChart(
            height: 96,
            ceiling: ceiling,
            gridLabels: _rateGrid(ceiling),
            axisLabels: _axis(context),
            series: [ChartSeries(received, down), ChartSeries(sent, up)],
          ),
          const SizedBox(height: 10),
          Row(
            children: [
              Expanded(
                child: RateValue(
                  color: down,
                  label: context.tr('hostDownload'),
                  bytes: latest?.received,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: RateValue(
                  color: up,
                  label: context.tr('hostUpload'),
                  bytes: latest?.sent,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// Aggregate read and write rates followed by one row per partition.
class DiskCard extends StatelessWidget {
  const DiskCard({super.key, required this.metrics, required this.history});
  final Map<String, dynamic> metrics;
  final MetricHistory history;

  @override
  Widget build(BuildContext context) {
    final disks = objects(metrics['disks']);
    final latest = history.samples.lastOrNull;
    final read = history.series((sample) => sample.read);
    final written = history.series((sample) => sample.written);
    final ceiling = rateCeiling([...read, ...written]);
    final readColor = metricColor(context, MetricTone.read);
    final writeColor = metricColor(context, MetricTone.write);
    return Surface(
      padding: _cardPadding,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          CardHeader(
            title: context.tr('hostMetricDisk'),
            detail: context
                .tr('hostPartitions')
                .replaceAll('{count}', '${disks.length}'),
          ),
          AreaChart(
            height: 86,
            ceiling: ceiling,
            gridLabels: _rateGrid(ceiling),
            axisLabels: _axis(context),
            series: [
              ChartSeries(read, readColor),
              ChartSeries(written, writeColor),
            ],
          ),
          const SizedBox(height: 10),
          Row(
            children: [
              Expanded(
                child: RateValue(
                  color: readColor,
                  label: context.tr('hostDiskRead'),
                  bytes: latest?.read,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: RateValue(
                  color: writeColor,
                  label: context.tr('hostDiskWrite'),
                  bytes: latest?.written,
                ),
              ),
            ],
          ),
          for (final disk in disks) _Partition(disk: disk, color: readColor),
        ],
      ),
    );
  }
}

class _Partition extends StatelessWidget {
  const _Partition({required this.disk, required this.color});
  final Map<String, dynamic> disk;
  final Color color;

  @override
  Widget build(BuildContext context) {
    final total = number(disk['total_bytes']);
    final used = total - number(disk['available_bytes']);
    final share = usedShare(disk) ?? 0;
    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.baseline,
            textBaseline: TextBaseline.alphabetic,
            children: [
              Expanded(
                child: Text.rich(
                  TextSpan(
                    text: text(disk['mount'], text(disk['name'])),
                    children: [
                      TextSpan(
                        text: '  ${text(disk['file_system'])}',
                        style: dimStyle(context),
                      ),
                    ],
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 14, fontFeatures: figures),
                ),
              ),
              const SizedBox(width: 8),
              Text(
                total > 0
                    ? '${byteRatio(used, total)}  ${percent(share)}'
                    : '—',
                style: mutedStyle(context),
              ),
            ],
          ),
          const SizedBox(height: 7),
          MeterBar(share: share, color: color),
        ],
      ),
    );
  }
}
