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
    test('separates online, connecting and offline hosts', () {
      expect(hostStatus(true, null), HostStatus.online);
      expect(hostStatus(false, null), HostStatus.connecting);
      expect(hostStatus(false, 'refused'), HostStatus.offline);
    });

    test('reads latency only for a known route', () {
      expect(linkLatency({'route': 'p2p', 'rtt_ms': 18.4}), 18.4);
      expect(linkLatency({'route': 'relay', 'rtt_ms': 86}), 86);
      expect(linkLatency({'route': 'unknown', 'rtt_ms': 5}), isNull);
      expect(linkLatency({'route': 'p2p', 'rtt_ms': -1}), isNull);
      expect(linkRoute({'route': 'lan'}), isNull);
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
    test('derives used and cache from available and free', () {
      final split = MemorySplit.read({
        'total_bytes': 64 * _gib,
        'available_bytes': 24 * _gib,
        'free_bytes': 4 * _gib,
      });
      expect(split.used, 40 * _gib);
      expect(split.cache, 20 * _gib);
      expect(split.share, .625);
    });

    test('clamps inconsistent counters', () {
      final split = MemorySplit.read({
        'total_bytes': 8,
        'available_bytes': 12,
        'free_bytes': 20,
      });
      expect(split.used, 0);
      expect(split.cache, 0);
      expect(MemorySplit.read({}).share, isNull);
    });
  });

  group('history', () {
    test('keeps the newest samples up to capacity', () {
      final history = MetricHistory(3);
      for (var value = 1; value <= 5; value++) {
        history.add(MetricSample(latency: value.toDouble()));
      }
      expect(history.series((sample) => sample.latency), [3, 4, 5]);
      expect(history.latencyRange, (4.0, 5.0));
      history.clear();
      expect(history.samples, isEmpty);
      expect(history.latencyRange, isNull);
    });

    test('reads utilization and summed disk rates', () {
      final sample = MetricSample.read({
        'cpu_basis_points': 2300,
        'memory': {'total_bytes': 100, 'available_bytes': 36, 'free_bytes': 10},
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
      expect(sample.cpu, .23);
      expect(sample.memory, .64);
      expect(sample.gpu, .2);
      expect(sample.received, 1200);
      expect(sample.sent, 340);
      expect(sample.read, 15);
      expect(sample.written, 3);
      expect(sample.latency, 21);
    });

    test('leaves missing sources empty', () {
      final sample = MetricSample.read({'disks': <Object>[]}, null);
      expect(sample.cpu, isNull);
      expect(sample.gpu, isNull);
      expect(sample.received, isNull);
      expect(sample.read, isNull);
    });
  });

  group('formatting', () {
    test('uses binary units with one decimal below 100', () {
      expect(formatBytes(512), '512 B');
      expect(formatBytes(340 * 1024), '340 KB');
      expect(formatBytes(1.2 * 1024 * 1024), '1.2 MB');
      expect(formatBytes(64 * _gib), '64 GB');
      expect(formatRate(3 * 1024 * 1024), '3 MB/s');
      expect(byteParts(26.9 * _gib), ('26.9', 'GB'));
    });

    test('shares the total unit in a ratio', () {
      expect(byteRatio(41.2 * _gib, 64 * _gib), '41.2 / 64 GB');
      expect(byteRatio(1.6 * 1024 * _gib, 3.6 * 1024 * _gib), '1.6 / 3.6 TB');
    });

    test('formats percentages and load', () {
      expect(percent(.234), '23%');
      expect(basisPercent(8640), '86.4%');
      expect(loadValue(214), '2.14');
    });

    test('picks the largest whole duration unit', () {
      expect(durationParts(6 * 86400 + 3600), ('hostDays', 6));
      expect(durationParts(3 * 3600 + 59), ('hostHours', 3));
      expect(durationParts(150), ('hostMinutes', 2));
    });

    test('rounds rate ceilings to readable steps', () {
      expect(rateCeiling([null, 0]), 1024);
      expect(rateCeiling([3.1 * 1024 * 1024]), 4 * 1024 * 1024);
      expect(rateCeiling([35.0 * 1024 * 1024]), 40 * 1024 * 1024);
      expect(rateCeiling([900]), 1024);
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

    test('sorts by CPU or memory and limits the result', () {
      expect(rankProcesses(processes, limit: 2).map((p) => p['name']), [
        'cargo',
        'rust-analyzer',
      ]);
      expect(
        rankProcesses(processes, byMemory: true).first['name'],
        'rust-analyzer',
      );
    });

    test('filters by name or exact PID', () {
      expect(rankProcesses(processes, query: 'RUST').map((p) => p['name']), [
        'rust-analyzer',
      ]);
      expect(rankProcesses(processes, query: '8820').single['name'], 'zsh');
      expect(rankProcesses(processes, query: '882'), isEmpty);
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
