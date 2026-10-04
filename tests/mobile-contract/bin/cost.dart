import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'overview.dart' show ready;
import 'usage.dart' show until;

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
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
  final first = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  final second = await controller.pair(
    ticket: env['SAILRY_SECOND_INVITATION']!,
  );
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
    addresses: [first, first, second],
    query: query,
  );
  final original = await ready(updates, 3);
  final total = original['summary']['totals'];
  check(
    total['generation']['responses'] == 3 &&
        total['generation']['output_tokens'] == 12 &&
        total['generation']['elapsed_us'] > 0,
    'overview combines measured output and duration without averaging per-node rates',
  );
  check(
    total['cost']['usd_micros'] == 250000 && total['cost']['responses'] == 2,
    'overview preserves partial price coverage and deduplicates Nodes',
  );
  final connection = await controller.connect(address: first);
  final single = await connection.watchUsage(query: query);
  final report = await until(single, (_) => true);
  check(
    report['totals']['cost']['usd_micros'] == 125000 &&
        report['totals']['cost']['responses'] == 1,
    'single Node exposes cost through the shared report',
  );
  await single.close();
  single.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  updates.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  updates = await controller.watchUsage(
    addresses: [first, second],
    query: query,
  );
  final restored = await ready(updates, 3);
  check(
    jsonEncode(restored['summary']) == jsonEncode(original['summary']),
    'reopened controller restores estimates without model execution',
  );
  await updates.close();
  updates.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
}
