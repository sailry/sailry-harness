import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/overview.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' as conversation;

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
}

Future<Map<String, dynamic>> ready(OverviewUpdates updates, int count) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      check(view['error'] == null, 'overview aggregation succeeds');
      if (view['complete'] == true &&
          view['summary']['totals']['responses'] == count)
        return view;
    }
  }).timeout(const Duration(seconds: 10));
}

Future<conversation.Fixture> create(Connection connection, String path) async {
  final project = await conversation.execute(connection, 'register_project', {
    'name': 'Overview fixture',
    'path': path,
  });
  final snapshot = await conversation.execute(connection, 'snapshot', null);
  return conversation.create(
    connection,
    project['data']['id'],
    snapshot['data']['worktrees'][0]['id'],
  );
}

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final firstAddress = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  final secondAddress = await controller.pair(
    ticket: env['SAILRY_SECOND_INVITATION']!,
  );
  final first = await controller.connect(address: firstAddress);
  final second = await controller.connect(address: secondAddress);
  final now = DateTime.now().millisecondsSinceEpoch;
  final query = jsonEncode({
    'start_ms': now - 86400000,
    'end_ms': now + 86400000,
    'dimension': 'model',
    'projects': [],
    'worktrees': [],
    'providers': [],
    'models': [],
  });
  var updates = await controller.watchUsage(
    addresses: [firstAddress, firstAddress, secondAddress],
    query: query,
  );
  final empty = await ready(updates, 0);
  check(
    (empty['sources'] as List).length == 2 &&
        empty['summary']['totals']['tokens'] == null,
    'empty reports retain unknown counters and deduplicate nodes',
  );
  final firstFixture = await create(first, env['SAILRY_PROJECT_PATH']!);
  final secondFixture = await create(
    second,
    env['SAILRY_SECOND_PROJECT_PATH']!,
  );
  final view = await ready(updates, 2);
  final summary = view['summary'];
  check(
    summary['totals']['tokens']['input'] == 16 &&
        summary['totals']['tokens']['output'] == 8,
    'shared overview combines reported usage once',
  );
  final groups = summary['groups'] as List;
  check(
    groups.length == 2 &&
        groups.map((group) => jsonEncode(group['node'])).toSet().length == 2,
    'matching model and provider identifiers retain distinct node ownership',
  );
  await first.close();
  first.dispose();
  await updates.refresh();
  await ready(updates, 2);
  await controller.close();
  var closed = false;
  try {
    await updates.next();
  } catch (_) {
    closed = true;
  }
  check(closed, 'controller closure cancels the complete observer tree');
  updates.dispose();
  second.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  updates = await controller.watchUsage(
    addresses: await controller.peers(),
    query: query,
  );
  final restored = await ready(updates, 2);
  check(
    jsonEncode(restored['summary']) == jsonEncode(summary),
    'controller restart restores the same aggregate without replay',
  );
  final restoredFirst = await controller.connect(address: firstAddress);
  final restoredSecond = await controller.connect(address: secondAddress);
  await conversation.restore(restoredFirst, firstFixture);
  await conversation.restore(restoredSecond, secondFixture);
  final pending = updates.next().then((_) => true, onError: (_) => true);
  await updates.close();
  await pending.timeout(const Duration(seconds: 2));
  closed = false;
  try {
    await updates.refresh();
  } catch (_) {
    closed = true;
  }
  check(closed, 'closed overview cannot refresh');
  updates.dispose();
  await controller.close();
  restoredFirst.dispose();
  restoredSecond.dispose();
  controller.dispose();
  print('Dart FFI multi-node usage and observer ownership passed');
}
