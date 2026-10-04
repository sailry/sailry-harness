import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, until;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final session = env['SAILRY_SESSION']!;
  final manual = env['SAILRY_MANUAL_CONTEXT'] == '1';
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  String? request;
  String? receipt;
  Map<String, dynamic>? finished;
  for (final text in [
    'Original requirement 中文 🙂 ' * 700,
    if (!manual) 'Recent context',
    if (!manual) 'Current task',
  ]) {
    request = await connection.prepare(
      command: jsonEncode({
        'kind': 'submit_turn',
        'data': {
          'session': session,
          'expected_revision': 1,
          'message': {'text': text, 'attachments': []},
        },
      }),
    );
    receipt = await connection.execute(request: request);
    final result = jsonDecode(receipt)['Ok'];
    check(result?['kind'] == 'queued_turn', 'turn was admitted');
    final turn = result['data']['id'];
    finished = await until(
      updates,
      (snapshot) => (snapshot['page']['runs'] as List).any(
        (run) => run['turn'] == turn && run['status'] == 'completed',
      ),
    );
  }
  if (manual) {
    request = await connection.prepare(
      command: jsonEncode({
        'kind': 'compact_context',
        'data': {'session': session, 'expected_revision': 1},
      }),
    );
    receipt = await connection.execute(request: request);
    final turn = jsonDecode(receipt)['Ok']['data'];
    check(turn['kind'] == 'compaction', 'admission identifies maintenance');
    finished = await until(
      updates,
      (snapshot) => (snapshot['page']['runs'] as List).any(
        (run) =>
            run['turn'] == turn['id'] &&
            run['kind'] == 'compaction' &&
            run['status'] == 'completed',
      ),
    );
    check(
      finished['snapshot']['statistics']['turns'] == 1,
      'maintenance does not add a user turn',
    );
    check(
      finished['snapshot']['statistics']['responses'] == 3,
      'summary usage remains counted',
    );
  }
  List summaries(Map<String, dynamic> view) =>
      (view['snapshot']['page']['entries'] as List)
          .expand((entry) => entry['parts'] as List)
          .where((part) => part['kind'] == 'compaction')
          .toList();
  final original = summaries(finished!);
  check(
    original.length == 1,
    'one durable summary arrives through the shared Client',
  );
  check(
    (original.single['data'] as String).contains('中文 🙂'),
    'summary text is preserved',
  );
  check(
    (finished['snapshot']['page']['entries'] as List).any(
      (entry) => (entry['parts'] as List).any(
        (part) =>
            part['kind'] == 'text' &&
            (part['data'] as String).contains('Original requirement'),
      ),
    ),
    'original history remains available',
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
    jsonEncode(summaries(restored)) == jsonEncode(original),
    'summaries survive controller restart',
  );
  check(
    await connection.execute(request: request!) == receipt,
    'retry preserves the original admission',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI context summary and reconnect passed');
}
