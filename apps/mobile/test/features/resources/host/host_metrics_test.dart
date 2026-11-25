import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/host/metric_history.dart';

const _gib = 1024 * 1024 * 1024;

Map<String, dynamic> process(String name, int pid, int cpu, int memory) => {
  'name': name,
  'pid': pid,
  'cpu_basis_points': cpu,
  'memory_bytes': memory,
};

void main() {
  group('status', () {
    test("separates connectivity from route latency", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "separates online, connecting and offline hosts",
          (caseName) {
            expect(hostStatus(true, null), HostStatus.online, reason: caseName);
            expect(
              hostStatus(false, null),
              HostStatus.connecting,
              reason: caseName,
            );
            expect(
              hostStatus(false, 'refused'),
              HostStatus.offline,
              reason: caseName,
            );
          },
        ),
        (
          "reads latency only for a known route",
          (caseName) {
            expect(
              linkLatency({'route': 'p2p', 'rtt_ms': 18.4}),
              18.4,
              reason: caseName,
            );
            expect(
              linkLatency({'route': 'relay', 'rtt_ms': 86}),
              86,
              reason: caseName,
            );
            expect(
              linkLatency({'route': 'unknown', 'rtt_ms': 5}),
              isNull,
              reason: caseName,
            );
            expect(
              linkLatency({'route': 'p2p', 'rtt_ms': -1}),
              isNull,
              reason: caseName,
            );
            expect(linkRoute({'route': 'lan'}), isNull, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('meter level', () {
    test('warns at 80 percent and alerts at 95 percent', () {
      expect(meterLevel(.79), MeterLevel.normal);
      expect(meterLevel(.8), MeterLevel.warning);
      expect(meterLevel(.949), MeterLevel.warning);
      expect(meterLevel(.95), MeterLevel.danger);
    });
  });

  group('memory split', () {
    test("derives bounded memory counters", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "derives used and cache from available and free",
          (caseName) {
            final split = MemorySplit.read({
              'total_bytes': 64 * _gib,
              'available_bytes': 24 * _gib,
              'free_bytes': 4 * _gib,
            });
            expect(split.used, 40 * _gib, reason: caseName);
            expect(split.cache, 20 * _gib, reason: caseName);
            expect(split.share, .625, reason: caseName);
          },
        ),
        (
          "clamps inconsistent counters",
          (caseName) {
            final split = MemorySplit.read({
              'total_bytes': 8,
              'available_bytes': 12,
              'free_bytes': 20,
            });
            expect(split.used, 0, reason: caseName);
            expect(split.cache, 0, reason: caseName);
            expect(MemorySplit.read({}).share, isNull, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('history', () {
    test("retains bounded samples and missing source semantics", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "keeps the newest samples up to capacity",
          (caseName) {
            final history = MetricHistory(3);
            for (var value = 1; value <= 5; value++) {
              history.add(MetricSample(latency: value.toDouble()));
            }
            expect(history.series((sample) => sample.latency), [
              3,
              4,
              5,
            ], reason: caseName);
            expect(history.latencyRange, (4.0, 5.0), reason: caseName);
            history.clear();
            expect(history.samples, isEmpty, reason: caseName);
            expect(history.latencyRange, isNull, reason: caseName);
          },
        ),
        (
          "reads utilization and summed disk rates",
          (caseName) {
            final sample = MetricSample.read({
              'cpu_basis_points': 2300,
              'memory': {
                'total_bytes': 100,
                'available_bytes': 36,
                'free_bytes': 10,
              },
              'gpus': [
                {'usage_basis_points': 1000},
                {'usage_basis_points': 3000},
                {'usage_basis_points': null},
              ],
              'network': {
                'received_bytes_per_sec': 1200,
                'transmitted_bytes_per_sec': 340,
              },
              'disks': [
                {
                  'io': {'read_bytes_per_sec': 10, 'written_bytes_per_sec': 2},
                },
                {'io': null},
                {
                  'io': {'read_bytes_per_sec': 5, 'written_bytes_per_sec': 1},
                },
              ],
            }, 21);
            expect(sample.cpu, .23, reason: caseName);
            expect(sample.memory, .64, reason: caseName);
            expect(sample.gpu, .2, reason: caseName);
            expect(sample.received, 1200, reason: caseName);
            expect(sample.sent, 340, reason: caseName);
            expect(sample.read, 15, reason: caseName);
            expect(sample.written, 3, reason: caseName);
            expect(sample.latency, 21, reason: caseName);
          },
        ),
        (
          "leaves missing sources empty",
          (caseName) {
            final sample = MetricSample.read({'disks': <Object>[]}, null);
            expect(sample.cpu, isNull, reason: caseName);
            expect(sample.gpu, isNull, reason: caseName);
            expect(sample.received, isNull, reason: caseName);
            expect(sample.read, isNull, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('formatting', () {
    test("formats units ratios rates and durations", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (
          formatBytes(512),
          '512 B',
          "uses binary units with one decimal below 100",
        ),
        (
          formatBytes(340 * 1024),
          '340 KB',
          "uses binary units with one decimal below 100",
        ),
        (
          formatBytes(1.2 * 1024 * 1024),
          '1.2 MB',
          "uses binary units with one decimal below 100",
        ),
        (
          formatBytes(64 * _gib),
          '64 GB',
          "uses binary units with one decimal below 100",
        ),
        (
          formatRate(3 * 1024 * 1024),
          '3 MB/s',
          "uses binary units with one decimal below 100",
        ),
        (
          byteParts(26.9 * _gib),
          ('26.9', 'GB'),
          "uses binary units with one decimal below 100",
        ),
        (
          byteRatio(41.2 * _gib, 64 * _gib),
          '41.2 / 64 GB',
          "shares the total unit in a ratio",
        ),
        (
          byteRatio(1.6 * 1024 * _gib, 3.6 * 1024 * _gib),
          '1.6 / 3.6 TB',
          "shares the total unit in a ratio",
        ),
        (percent(.234), '23%', "formats percentages and load"),
        (basisPercent(8640), '86.4%', "formats percentages and load"),
        (loadValue(214), '2.14', "formats percentages and load"),
        (
          durationParts(6 * 86400 + 3600),
          ('hostDays', 6),
          "picks the largest whole duration unit",
        ),
        (
          durationParts(3 * 3600 + 59),
          ('hostHours', 3),
          "picks the largest whole duration unit",
        ),
        (
          durationParts(150),
          ('hostMinutes', 2),
          "picks the largest whole duration unit",
        ),
        (
          rateCeiling([null, 0]),
          1024,
          "rounds rate ceilings to readable steps",
        ),
        (
          rateCeiling([3.1 * 1024 * 1024]),
          4 * 1024 * 1024,
          "rounds rate ceilings to readable steps",
        ),
        (
          rateCeiling([35.0 * 1024 * 1024]),
          40 * 1024 * 1024,
          "rounds rate ceilings to readable steps",
        ),
        (rateCeiling([900]), 1024, "rounds rate ceilings to readable steps"),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  group('uptime', () {
    test('advances by the node-clock interval since inspection', () {
      final info = {
        'sampled_at_ms': 1000,
        'host_uptime_secs': 100,
        'node_uptime_secs': 10,
      };
      expect(currentUptimes(info, {'sampled_at_ms': 61000}), (160, 70));
      expect(currentUptimes(info, {'sampled_at_ms': 0}), (100, 10));
      expect(currentUptimes(info, {}), (100, 10));
    });
  });

  group('processes', () {
    final processes = [
      process('zsh', 8820, 20, 8),
      process('cargo', 48211, 8640, 1800),
      process('rust-analyzer', 5530, 980, 3200),
      process('idle', 1, 0, 1),
    ];

    test("sorts limits and filters process rows", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "sorts by CPU or memory and limits the result",
          (caseName) {
            expect(
              rankProcesses(processes, limit: 2).map((p) => p['name']),
              ['cargo', 'rust-analyzer'],
              reason: caseName,
            );
            expect(
              rankProcesses(processes, byMemory: true).first['name'],
              'rust-analyzer',
              reason: caseName,
            );
          },
        ),
        (
          "filters by name or exact PID",
          (caseName) {
            expect(
              rankProcesses(processes, query: 'RUST').map((p) => p['name']),
              ['rust-analyzer'],
              reason: caseName,
            );
            expect(
              rankProcesses(processes, query: '8820').single['name'],
              'zsh',
              reason: caseName,
            );
            expect(
              rankProcesses(processes, query: '882'),
              isEmpty,
              reason: caseName,
            );
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('projects', () {
    Map<String, dynamic> session(String project, int finished) => {
      'project': project,
      'activity': {
        'run': {'finished_ms': finished},
      },
    };

    test('orders by the latest session run', () {
      final projects = [
        {'id': 'a'},
        {'id': 'b'},
        {'id': 'c'},
        {'id': 'd'},
      ];
      final ordered = recentProjects(projects, [
        session('a', 10),
        session('c', 30),
        session('a', 20),
        {'project': 'd'},
      ]);
      expect(ordered.map((project) => project['id']), ['c', 'a', 'b', 'd']);
    });
  });
}
