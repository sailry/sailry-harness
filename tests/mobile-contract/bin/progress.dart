import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show check, execute, until;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final session = env['SAILRY_SESSION']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final original = (snapshot['sessions'] as List).singleWhere(
    (item) => item['id'] == session,
  );
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session,
        'expected_revision': original['revision'],
        'message': {'text': 'Report task progress 中文 🙂', 'attachments': []},
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  check(
    jsonDecode(receipt)['Ok']?['kind'] == 'queued_turn',
    'turn was admitted',
  );
  final finished = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  final calls = finished['calls'] as List;
  check(calls.length == 2, 'both progress updates are retained');
  check(
    calls.first['progress']['steps'][1]['state'] == 'in_progress',
    'earlier progress is immutable',
  );
  check(
    calls.last['progress']['steps'][1]['state'] == 'completed',
    'current progress arrives from shared Client',
  );
  check(
    calls.last['progress']['title'] == 'Task 中文 🙂',
    'Unicode progress is preserved',
  );
  for (final call in calls) {
    check(
      call['grouping'] == 'standalone',
      'Node grouping reaches the mobile Client',
    );
    final expected = part(finished, call['response'])['result']['progress'];
    final actual = call['progress'];
    check(
      actual['title'] == expected['title'] &&
          (actual['steps'] as List).length ==
              (expected['steps'] as List).length &&
          List.generate(
            (actual['steps'] as List).length,
            (index) =>
                actual['steps'][index]['description'] ==
                    expected['steps'][index]['description'] &&
                actual['steps'][index]['state'] ==
                    expected['steps'][index]['state'],
          ).every((value) => value),
      'typed progress matches its canonical result',
    );
  }
  check(
    (finished['snapshot']['page']['questions'] as List).isEmpty,
    'progress does not ask for approval',
  );
  Future<void> close() async {
    await updates.close();
    updates.dispose();
    await connection.close();
    connection.dispose();
    await controller.close();
    controller.dispose();
  }

  await close();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final restored = await until(updates, (_) => true);
  check(
    jsonEncode(restored['calls']) == jsonEncode(calls),
    'progress survives controller restart',
  );
  check(
    await connection.execute(request: request) == receipt,
    'retry preserves the original admission',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI task progress and reconnect passed');
}
