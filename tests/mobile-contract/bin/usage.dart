import 'dart:convert';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/usage.dart';

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

Future<Map<String, dynamic>> until(
  UsageUpdates updates,
  bool Function(Map<String, dynamic>) ready,
) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      if (view['error'] != null) {
        throw StateError('Usage observer failed: ${view["error"]}');
      }
      final report = view['report'] as Map<String, dynamic>?;
      if (view['connected'] == true &&
          view['refreshing'] == false &&
          report != null &&
          ready(report)) {
        return report;
      }
    }
  }).timeout(const Duration(seconds: 10));
}

class Fixture {
  final String query;
  final UsageUpdates updates;
  late Map<String, dynamic> report;

  Fixture(this.query, this.updates);
}

Future<Fixture> create(Connection connection, String project) async {
  final now = DateTime.now().millisecondsSinceEpoch;
  final query = jsonEncode({
    'start_ms': now - const Duration(days: 1).inMilliseconds,
    'end_ms': now + const Duration(days: 1).inMilliseconds,
    'dimension': 'model',
    'projects': [project],
    'worktrees': [],
    'providers': [],
    'models': [],
  });
  final updates = await connection.watchUsage(query: query);
  final empty = await until(updates, (_) => true);
  check(
    empty['totals']['responses'] == 0 && empty['totals']['tokens'] == null,
    'empty global report retains unknown usage',
  );
  check(
    (empty['days'] as List).isNotEmpty &&
        (empty['recent'] as List).isNotEmpty &&
        (empty['groups'] as List).isEmpty &&
        (empty['resources'] as List).isEmpty,
    'empty time buckets cross the shared report bridge',
  );
  return Fixture(query, updates);
}

Future<void> complete(Fixture fixture) async {
  final report = await until(
    fixture.updates,
    (report) => report['totals']['responses'] == 1,
  );
  final tokens = report['totals']['tokens'];
  check(
    tokens['input'] == 8 &&
        tokens['output'] == 4 &&
        tokens['cached_input'] == 0 &&
        tokens['reasoning'] == 0,
    'global usage counts reported tokens once over FFI',
  );
  final group = (report['groups'] as List).single;
  final resources = report['resources'] as List;
  check(
    resources.length == 4 &&
        resources.any(
          (resource) =>
              resource['kind'] == 'model' &&
              resource['data']['model'] == 'ffi-fixture',
        ),
    'historical filter resources cross the shared report bridge',
  );
  check(
    group['key']['kind'] == 'model' &&
        group['key']['data']['model'] == 'ffi-fixture' &&
        jsonEncode(group['metrics']) == jsonEncode(report['totals']),
    'global grouping follows the frozen model',
  );
  await fixture.updates.refresh();
  fixture.report = await until(
    fixture.updates,
    (next) => next['cursor'] >= report['cursor'],
  );
  check(
    jsonEncode(fixture.report['totals']) == jsonEncode(report['totals']),
    'manual report refresh preserves canonical totals',
  );
  await close(fixture.updates);
}

Future<void> restore(Connection connection, Fixture fixture) async {
  final updates = await connection.watchUsage(query: fixture.query);
  final report = await until(updates, (_) => true);
  for (final key in [
    'node',
    'query',
    'totals',
    'days',
    'recent',
    'groups',
    'resources',
  ]) {
    check(
      jsonEncode(report[key]) == jsonEncode(fixture.report[key]),
      'reopened controller restores global usage $key',
    );
  }
  await close(updates);
}

Future<void> close(UsageUpdates updates) async {
  // A final coalesced update may race cancellation; both outcomes must wake next.
  final pending = updates.next().then((_) => true, onError: (_) => true);
  await updates.close();
  await pending.timeout(const Duration(seconds: 2));
  for (final action in [() => updates.next(), () => updates.refresh()]) {
    var closed = false;
    try {
      await action();
    } catch (_) {
      closed = true;
    }
    check(closed, 'closed usage observer rejects further work');
  }
  updates.dispose();
}
