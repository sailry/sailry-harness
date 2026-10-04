import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;

Future<Map<String, dynamic>> observed(
  Updates updates,
  bool Function(Map<String, dynamic>) ready,
) => Future(() async {
  while (true) {
    final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
    check(view['error'] == null, 'subscription remains healthy');
    if (view['connected'] == true && ready(view)) return view;
  }
}).timeout(const Duration(seconds: 10));

List<String> ids(Map<String, dynamic> view) => [
  for (final session in view['snapshot']['sessions']) session['id'] as String,
];

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final controller = await Controller.open(
    path: env['SAILRY_CONTROLLER_PROFILE']!,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  final connection = await controller.connect(address: address);
  final sessions = List<String>.from(jsonDecode(env['SAILRY_SESSIONS']!));
  var watch = await connection.watch();
  final initial = await observed(watch, (_) => true);
  check(
    jsonEncode(ids(initial)) == jsonEncode(sessions.reversed.toList()),
    'newest created session first',
  );
  File(env['SAILRY_READY']!).writeAsStringSync('ready');
  final order = [sessions[0], sessions[2], sessions[1]];
  await observed(watch, (view) => jsonEncode(ids(view)) == jsonEncode(order));
  await execute(connection, 'rename_session', {
    'session': sessions[1],
    'expected_revision': 1,
    'title': 'Renamed without moving',
  });
  final changed = await observed(
    watch,
    (view) => (view['snapshot']['sessions'] as List).any(
      (session) => session['activity']['title'] == 'Renamed without moving',
    ),
  );
  check(
    jsonEncode(ids(changed)) == jsonEncode(order),
    'updates retain desktop order',
  );
  watch.close();
  watch = await connection.watch();
  final recovered = await observed(watch, (_) => true);
  check(
    jsonEncode(ids(recovered)) == jsonEncode(order),
    'new mobile subscription retains order',
  );
  watch.close();
  await controller.close();
}
