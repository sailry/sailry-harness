import 'dart:math' as math;

import '../../../runtime/json.dart';
import '../../conversations/live/recency.dart';

/// Node only reports rates when consecutive detailed reads are under five
/// seconds apart, so polling stays well inside that window.
const samplePeriod = Duration(seconds: 3);

/// Twenty-one samples at [samplePeriod] span the last minute.
const historyCapacity = 21;

/// Visible process rows after sorting and filtering.
const processLimit = 12;

enum HostStatus { online, connecting, offline }

/// A disconnected observer without a fault is still establishing its link.
HostStatus hostStatus(bool connected, Object? error) => connected
    ? HostStatus.online
    : error == null
    ? HostStatus.connecting
    : HostStatus.offline;

enum MeterLevel { normal, warning, danger }

MeterLevel meterLevel(double share) => share >= .95
    ? MeterLevel.danger
    : share >= .8
    ? MeterLevel.warning
    : MeterLevel.normal;

/// Selected route, or null when the link reports neither P2P nor relay.
String? linkRoute(Map<String, dynamic> transport) {
  final route = transport['route'];
  return route == 'p2p' || route == 'relay' ? route as String : null;
}

/// Round-trip time of a known route in milliseconds.
double? linkLatency(Map<String, dynamic> transport) {
  final rtt = transport['rtt_ms'];
  return linkRoute(transport) != null && rtt is num && rtt.isFinite && rtt >= 0
      ? rtt.toDouble()
      : null;
}

double? _share(Object? basisPoints) =>
    basisPoints is num ? (basisPoints / 10000).clamp(0, 1).toDouble() : null;

/// Memory segments: used is total minus available; cache is available minus free.
class MemorySplit {
  const MemorySplit({
    required this.total,
    required this.used,
    required this.cache,
  });

  factory MemorySplit.read(Map<String, dynamic> memory) {
    final total = number(memory['total_bytes']).toDouble();
    final available = number(
      memory['available_bytes'],
    ).toDouble().clamp(0, total).toDouble();
    final free = number(
      memory['free_bytes'],
      available,
    ).toDouble().clamp(0, available).toDouble();
    return MemorySplit(
      total: total,
      used: total - available,
      cache: available - free,
    );
  }

  final double total;
  final double used;
  final double cache;

  double? get share => total > 0 ? used / total : null;
  double? get cacheShare => total > 0 ? cache / total : null;
}

/// Used fraction of a disk or memory record with total and available bytes.
double? usedShare(Map<String, dynamic> value) {
  final total = number(value['total_bytes']);
  if (total <= 0) return null;
  return ((total - number(value['available_bytes'])) / total)
      .clamp(0, 1)
      .toDouble();
}

/// One locally retained observation of the selected host.
class MetricSample {
  const MetricSample({
    this.cpu,
    this.memory,
    this.gpu,
    this.received,
    this.sent,
    this.read,
    this.written,
    this.latency,
  });

  factory MetricSample.read(Map<String, dynamic> metrics, double? latency) {
    final network = metrics['network'] is Map
        ? object(metrics['network'])
        : null;
    final gpus = objects(
      metrics['gpus'],
    ).map((gpu) => _share(gpu['usage_basis_points'])).nonNulls.toList();
    final io = objects(
      metrics['disks'],
    ).map((disk) => disk['io']).whereType<Map>().map(object).toList();
    double? sum(String key) => io.isEmpty
        ? null
        : io.fold<double>(0, (total, rate) => total + number(rate[key]));
    return MetricSample(
      cpu: _share(metrics['cpu_basis_points']),
      memory: MemorySplit.read(object(metrics['memory'])).share,
      gpu: gpus.isEmpty ? null : gpus.reduce((a, b) => a + b) / gpus.length,
      received: network == null
          ? null
          : number(network['received_bytes_per_sec']).toDouble(),
      sent: network == null
          ? null
          : number(network['transmitted_bytes_per_sec']).toDouble(),
      read: sum('read_bytes_per_sec'),
      written: sum('written_bytes_per_sec'),
      latency: latency,
    );
  }

  /// Utilization fractions in 0..1.
  final double? cpu;
  final double? memory;
  final double? gpu;

  /// Bytes per second.
  final double? received;
  final double? sent;
  final double? read;
  final double? written;

  /// Milliseconds.
  final double? latency;
}

/// Fixed-size ring of the most recent samples, oldest first.
class MetricHistory {
  MetricHistory([this.capacity = historyCapacity]);

  final int capacity;
  final _samples = <MetricSample>[];

  List<MetricSample> get samples => List.unmodifiable(_samples);

  void add(MetricSample sample) {
    _samples.add(sample);
    if (_samples.length > capacity) {
      _samples.removeRange(0, _samples.length - capacity);
    }
  }

  void clear() => _samples.clear();

  List<double?> series(double? Function(MetricSample sample) pick) =>
      _samples.map(pick).toList();

  /// Average and peak latency in milliseconds over the retained samples.
  (double, double)? get latencyRange {
    final values = _samples.map((sample) => sample.latency).nonNulls.toList();
    if (values.isEmpty) return null;
    return (
      values.reduce((a, b) => a + b) / values.length,
      values.reduce(math.max),
    );
  }
}

const _units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];

int _unitIndex(num bytes) {
  var index = 0;
  var value = bytes.abs().toDouble();
  while (value >= 1024 && index < _units.length - 1) {
    value /= 1024;
    index++;
  }
  return index;
}

String _scaled(num bytes, int unit) {
  final value = bytes / math.pow(1024, unit);
  final digits = value >= 100 || unit == 0 ? 0 : 1;
  final text = value.toStringAsFixed(digits);
  return text.endsWith('.0') ? text.substring(0, text.length - 2) : text;
}

/// Binary byte count split into number and unit, such as ("41.2", "GB").
(String, String) byteParts(num bytes) {
  final unit = _unitIndex(bytes);
  return (_scaled(bytes, unit), _units[unit]);
}

String formatBytes(num bytes) {
  final (value, unit) = byteParts(bytes);
  return '$value $unit';
}

String formatRate(num bytes) => '${formatBytes(bytes)}/s';

/// Used and total in the total's unit, such as "41.2 / 64 GB".
String byteRatio(num used, num total) {
  final unit = _unitIndex(total);
  return '${_scaled(used, unit)} / ${_scaled(total, unit)} ${_units[unit]}';
}

String percent(double share) => '${(share * 100).round()}%';

/// Hundredths of a percent as a percentage with one decimal.
String basisPercent(num basisPoints) =>
    '${(basisPoints / 100).toStringAsFixed(1)}%';

/// Load average in hundredths, such as 214 -> "2.14".
String loadValue(num hundredths) => (hundredths / 100).toStringAsFixed(2);

/// Rounded rate-chart ceiling whose half is still a readable value.
double rateCeiling(Iterable<double?> values) {
  final peak = values.nonNulls.fold<double>(0, math.max);
  if (peak <= 0) return 1024;
  var unit = 1.0;
  while (peak / unit >= 1024) {
    unit *= 1024;
  }
  const steps = [1, 2, 4, 6, 8, 10, 20, 40, 60, 80, 100, 200, 400, 600, 800];
  final scaled = peak / unit;
  final step = steps.firstWhere((step) => step >= scaled, orElse: () => 1024);
  return step * unit;
}

/// Largest whole duration unit as a localization key and count.
(String, int) durationParts(num seconds) {
  final value = seconds.toInt();
  if (value >= 86400) return ('hostDays', value ~/ 86400);
  if (value >= 3600) return ('hostHours', value ~/ 3600);
  return ('hostMinutes', value ~/ 60);
}

/// Advances inspected uptimes by the node-clock interval to the latest metrics.
(int, int) currentUptimes(
  Map<String, dynamic> info,
  Map<String, dynamic> metrics,
) {
  final inspected = number(info['sampled_at_ms']);
  final sampled = number(metrics['sampled_at_ms'], inspected);
  final elapsed = math.max(0, (sampled - inspected) ~/ 1000);
  return (
    number(info['host_uptime_secs']).toInt() + elapsed,
    number(info['node_uptime_secs']).toInt() + elapsed,
  );
}

/// Top processes by CPU or memory, filtered by name or PID within the list.
List<Map<String, dynamic>> rankProcesses(
  List<Map<String, dynamic>> processes, {
  String query = '',
  bool byMemory = false,
  int limit = processLimit,
}) {
  final needle = query.trim().toLowerCase();
  final matches = needle.isEmpty
      ? processes.toList()
      : processes
            .where(
              (process) =>
                  text(process['name']).toLowerCase().contains(needle) ||
                  '${process['pid']}' == needle,
            )
            .toList();
  num key(Map<String, dynamic> process) => byMemory
      ? number(process['memory_bytes'])
      : number(process['cpu_basis_points'], -1);
  matches.sort((a, b) => key(b).compareTo(key(a)));
  return matches.take(limit).toList();
}

/// Projects ordered by their sessions' latest run, newest first.
List<Map<String, dynamic>> recentProjects(
  List<Map<String, dynamic>> projects,
  List<Map<String, dynamic>> sessions,
) {
  final latest = <Object?, int>{};
  for (final session in sessions) {
    final time = runTime(session)?.millisecondsSinceEpoch;
    if (time == null) continue;
    final project = session['project'];
    latest[project] = math.max(latest[project] ?? 0, time);
  }
  final indexed = projects.indexed.toList()
    ..sort((a, b) {
      final order = (latest[b.$2['id']] ?? -1).compareTo(
        latest[a.$2['id']] ?? -1,
      );
      return order != 0 ? order : a.$1.compareTo(b.$1);
    });
  return [for (final (_, project) in indexed) project];
}
