import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'approvals.dart' show part;

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
  final initial = await until(updates, (_) => true);
  check(
    (initial['calls'] as List).isEmpty,
    'a session starts without goal calls',
  );
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
        'message': {'text': '/goal Verify the result 中文 🙂', 'attachments': []},
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  check(
    jsonDecode(receipt)['Ok']?['kind'] == 'queued_turn',
    'turn was admitted',
  );
  final first = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  final calls = first['calls'] as List;
  check(
    calls.length == 3,
    'the model reads the goal, verifies evidence and completes it',
  );
  final originalGoal = part(first, calls.first['response'])['result']['goal'];
  final goal = part(first, calls.last['response'])['result']['goal'];
  check(
    originalGoal['state'] == 'active' &&
        originalGoal['revision'] == '1' &&
        goal['state'] == 'completed' &&
        goal['revision'] == '2' &&
        originalGoal['id'] == goal['id'],
    'package-owned revisions arrive through canonical results',
  );
  check(
    goal['description'] == 'Verify the result 中文 🙂',
    'original intent is preserved',
  );
  check(
    calls.last['resolved']['label']['label'] == 'Update goal' &&
        calls.last['content']['blocks'][0]['kind'] == 'notice' &&
        calls.last['content']['blocks'][1]['path'] == '/goal/description',
    'the native message projection preserves goal status and description',
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
    'original goal results survive controller restart',
  );
  check(
    await connection.execute(request: request) == receipt,
    'retry preserves the original admission',
  );
  final completion = await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': original['revision'],
    'message': {
      'text': '/goal Verify the revised result 中文 🙂',
      'attachments': [],
    },
  });
  final finished = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) =>
          run['turn'] == completion['data']['id'] &&
          run['status'] == 'completed',
    ),
  );
  final completed = part(
    finished,
    (finished['calls'] as List).last['response'],
  )['result']['goal'];
  check(
    completed['id'] != goal['id'] &&
        completed['revision'] == '4' &&
        completed['state'] == 'completed' &&
        completed['description'] == 'Verify the revised result 中文 🙂',
    'only an explicit new request replaces the completed goal',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI goal revisions and reconnect passed');
}
